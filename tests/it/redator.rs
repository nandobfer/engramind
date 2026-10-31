//! Redator, fila de jobs, segredos e `llm_calls` (006), contra LLMs roteirizados locais.

use std::{sync::Arc, time::Duration};

use axum::http::StatusCode;
use chrono::Utc;
use engramind::core::redator::guard::{GuardFuture, MeaningGuard};
use serde_json::{Value, json};

use crate::support::{
    ADMIN_TOKEN, TestContext,
    llm::{ScriptedLlm, anthropic, gemini, openai},
};

const CHANNEL: &str = "eng/global";
const TEXT: &str =
    "Para rodar os testes do servico localmente use cargo nextest com o filtro do modulo afetado";
const API_KEY: &str = "sk-segredo-do-provedor-de-teste";

/// Guarda de sentido determinística: sempre a mesma similaridade.
struct FixedGuard(f64);

impl MeaningGuard for FixedGuard {
    fn similarity<'a>(&'a self, _original: &'a str, _revised: &'a str) -> GuardFuture<'a> {
        let similarity = self.0;
        Box::pin(async move { Some(similarity) })
    }
}

async fn put_settings(context: &TestContext, settings: Value) {
    let (status, body) = context
        .call("PUT", "/v1/settings", Some(ADMIN_TOKEN), Some(settings))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn set_key(context: &TestContext, value: Value) -> (StatusCode, Value) {
    context
        .call(
            "PUT",
            "/v1/secrets/redator.api_key",
            Some(ADMIN_TOKEN),
            Some(json!({ "value": value })),
        )
        .await
}

/// Redator OpenAI-compatível apontado para o LLM roteirizado.
async fn configure(context: &TestContext, llm: &ScriptedLlm, extra: Value) {
    let mut settings = json!({
        "redator.provider": "openai_compatible",
        "redator.model": "modelo-principal",
        "redator.base_url": format!("{}/v1", llm.url),
        "redator.timeout_ms": 2000,
    });
    for (key, value) in extra.as_object().expect("extra") {
        settings[key] = value.clone();
    }
    put_settings(context, settings).await;
    let (status, body) = set_key(context, json!(API_KEY)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn setup() -> (TestContext, ScriptedLlm) {
    let context = TestContext::new().await;
    context.create_channel(CHANNEL).await;
    (context, ScriptedLlm::start().await)
}

async fn memory(context: &TestContext, id: &str) -> Value {
    let (status, body) = context
        .call(
            "GET",
            &format!("/v1/memories/{id}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn create(context: &TestContext, text: &str) -> String {
    let body = context.create_memory(ADMIN_TOKEN, CHANNEL, text).await;
    body["memory"]["id"].as_str().expect("id").to_owned()
}

async fn jobs(context: &TestContext, query: &str) -> Vec<Value> {
    let (status, body) = context
        .call("GET", &format!("/v1/jobs{query}"), Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["jobs"].as_array().expect("jobs").clone()
}

async fn redact_jobs(context: &TestContext, memory: &str) -> Vec<Value> {
    jobs(context, "?kind=redact")
        .await
        .into_iter()
        .filter(|job| job["payload"]["memory_id"] == memory)
        .collect()
}

async fn llm_calls(context: &TestContext, query: &str) -> Vec<Value> {
    let (status, body) = context
        .call(
            "GET",
            &format!("/v1/llm-calls{query}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["calls"].as_array().expect("calls").clone()
}

async fn channel_version(context: &TestContext) -> i64 {
    let (_, body) = context
        .call("GET", "/v1/channels/eng%2Fglobal", Some(ADMIN_TOKEN), None)
        .await;
    body["version"].as_i64().expect("version")
}

async fn context_version(context: &TestContext) -> String {
    let (status, body) = context
        .call("GET", "/v1/context", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["version"].as_str().expect("version").to_owned()
}

async fn run(context: &TestContext) -> usize {
    context.core.run_redact_jobs(Utc::now()).await
}

#[tokio::test]
async fn the_backlog_gets_titles_without_rewriting_the_text_while_there_is_no_guard() {
    let (context, llm) = setup().await;
    // Escrita antes de o Redator existir: o job espera na fila (backlog desde a 004).
    let id = create(&context, TEXT).await;
    let before = memory(&context, &id).await;
    assert_eq!(before["title_state"], "pending");
    let (channel_before, context_before) = (
        channel_version(&context).await,
        context_version(&context).await,
    );

    configure(&context, &llm, json!({})).await;
    llm.push(
        200,
        openai(json!({ "title": "Testes locais com cargo nextest", "text": "Use cargo nextest." })),
    );
    assert_eq!(run(&context).await, 1);

    let after = memory(&context, &id).await;
    assert_eq!(after["title"], "Testes locais com cargo nextest");
    assert_eq!(after["title_state"], "ready");
    assert_eq!(after["text"], TEXT, "sem guarda de sentido o texto fica");
    assert_eq!(
        after["version"], before["version"],
        "titulo nao muda a versao"
    );
    assert!(channel_version(&context).await > channel_before);
    assert_ne!(context_version(&context).await, context_before);
    let (_, history) = context
        .call(
            "GET",
            &format!("/v1/memories/{id}/history"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(history["revisions"].as_array().expect("revisions").len(), 1);

    let job = &redact_jobs(&context, &id).await[0];
    assert_eq!(job["state"], "done");
    assert_eq!(job["outcome"], "title_only");
    assert_eq!(job["attempts"], 1);

    let received = &llm.received()[0];
    assert_eq!(received.path, "/v1/chat/completions");
    assert_eq!(
        received.headers["authorization"],
        format!("Bearer {API_KEY}").as_str()
    );
    assert_eq!(received.body["model"], "modelo-principal");
    assert_eq!(received.body["response_format"]["type"], "json_schema");
    let system = received.body["messages"][0]["content"]
        .as_str()
        .expect("system");
    let user = received.body["messages"][1]["content"]
        .as_str()
        .expect("user");
    assert!(!system.contains(TEXT));
    assert!(user.starts_with("<<<MEMORIA-") && user.contains(TEXT));
    assert!(user.trim_end().ends_with(">>>"));

    let calls = llm_calls(&context, "?purpose=redact").await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["outcome"], "success");
    assert_eq!(calls[0]["provider"], "openai_compatible");
    assert_eq!(calls[0]["input_tokens"], 120);
    assert_eq!(calls[0]["output_tokens"], 30);
    assert_eq!(calls[0]["cost_usd"], 0.00042);
    assert!(!calls[0].to_string().contains("nextest"));
    let audit = context.count(
        "SELECT COUNT(*) FROM audit_log WHERE action = 'memory.redact' AND app_id = 'system'
         AND agent_id = 'redator'",
    );
    assert_eq!(audit, 1);
}

#[tokio::test]
async fn a_revised_text_that_keeps_the_meaning_becomes_a_redator_revision() {
    let (context, llm) = setup().await;
    context.core.set_meaning_guard(Arc::new(FixedGuard(0.95)));
    configure(&context, &llm, json!({})).await;
    let id = create(&context, TEXT).await;
    let revised = "Rode os testes locais com cargo nextest e o filtro do modulo afetado.";
    llm.push(
        200,
        openai(json!({ "title": "Testes locais", "text": revised })),
    );
    run(&context).await;

    let after = memory(&context, &id).await;
    assert_eq!(after["text"], revised);
    assert_eq!(after["title"], "Testes locais");
    assert_eq!(after["version"], 2);
    assert_eq!(after["embedding_state"], "pending");
    let (_, history) = context
        .call(
            "GET",
            &format!("/v1/memories/{id}/history"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    let revision = &history["revisions"][0];
    assert_eq!(revision["version"], 2);
    assert_eq!(revision["reason"], "redact");
    assert_eq!(revision["author"]["app_id"], "system");
    assert_eq!(revision["author"]["agent_id"], "redator");
    assert_eq!(
        history["revisions"][1]["text"], TEXT,
        "o original fica no historico"
    );
    assert_eq!(redact_jobs(&context, &id).await[0]["outcome"], "applied");
    let embeds = jobs(&context, "?kind=embed").await;
    assert_eq!(embeds.len(), 2, "embed da escrita e do texto revisado");
    assert_eq!(embeds[0]["payload"]["version"], 2);
    assert_eq!(
        embeds[0]["state"], "queued",
        "embed so roda a partir da 007"
    );
    // A revisão do Redator não enfileira outro `redact`.
    assert_eq!(redact_jobs(&context, &id).await.len(), 1);
    assert_eq!(run(&context).await, 0);
}

#[tokio::test]
async fn the_meaning_guard_duplicates_and_rewrite_text_keep_the_original_text() {
    let (context, llm) = setup().await;
    context.core.set_meaning_guard(Arc::new(FixedGuard(0.5)));
    configure(&context, &llm, json!({})).await;
    let id = create(&context, TEXT).await;
    llm.push(
        200,
        openai(json!({ "title": "Testes", "text": "Outro sentido completamente." })),
    );
    run(&context).await;
    let after = memory(&context, &id).await;
    assert_eq!(after["text"], TEXT);
    assert_eq!(after["title"], "Testes");
    assert_eq!(redact_jobs(&context, &id).await[0]["outcome"], "title_only");

    // Texto revisado igual a outra memória do canal: só o título.
    context.core.set_meaning_guard(Arc::new(FixedGuard(1.0)));
    let other = create(&context, "Deploy acontece no merge para a main").await;
    llm.push(200, openai(json!({ "title": "Deploy", "text": TEXT })));
    run(&context).await;
    assert_eq!(
        memory(&context, &other).await["text"],
        "Deploy acontece no merge para a main"
    );

    // Sem revisão de texto no canal: o esquema pede só o título.
    let (status, body) = context
        .call(
            "PUT",
            "/v1/channels/eng%2Fglobal/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "redator.rewrite_text": false, "redator.instructions": "Titulos em caixa baixa." })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let third = create(&context, "Logs nunca levam o texto das memorias").await;
    llm.push(200, openai(json!({ "title": "logs sem texto" })));
    run(&context).await;
    assert_eq!(memory(&context, &third).await["title"], "logs sem texto");
    let request = &llm.received()[2].body;
    let schema = &request["response_format"]["json_schema"]["schema"];
    assert_eq!(schema["required"], json!(["title"]));
    let system = request["messages"][0]["content"].as_str().expect("system");
    assert!(system.ends_with("Titulos em caixa baixa.\n"));
}

#[tokio::test]
async fn anthropic_and_gemini_adapters_send_their_own_requests() {
    let (context, llm) = setup().await;
    configure(&context, &llm, json!({})).await;
    put_settings(
        &context,
        json!({ "redator.provider": "anthropic", "redator.base_url": llm.url }),
    )
    .await;
    let first = create(&context, TEXT).await;
    llm.push(
        200,
        anthropic(json!({ "title": "Via Anthropic", "text": TEXT })),
    );
    run(&context).await;
    assert_eq!(memory(&context, &first).await["title"], "Via Anthropic");
    let request = &llm.received()[0];
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(request.headers["x-api-key"], API_KEY);
    assert_eq!(request.headers["anthropic-version"], "2023-06-01");
    assert_eq!(request.body["tool_choice"]["type"], "tool");
    assert_eq!(
        request.body["tools"][0]["name"],
        request.body["tool_choice"]["name"]
    );

    put_settings(&context, json!({ "redator.provider": "gemini" })).await;
    let second = create(&context, "Segredos ficam cifrados com a SECRETS_KEY").await;
    llm.push(
        200,
        gemini(json!({ "title": "Via Gemini", "text": "Segredos cifrados." })),
    );
    run(&context).await;
    assert_eq!(memory(&context, &second).await["title"], "Via Gemini");
    let request = &llm.received()[1];
    assert_eq!(
        request.path,
        "/v1beta/models/modelo-principal:generateContent"
    );
    assert_eq!(request.headers["x-goog-api-key"], API_KEY);
    assert_eq!(
        request.body["generationConfig"]["responseSchema"]["type"],
        "OBJECT"
    );
    let calls = llm_calls(&context, "").await;
    assert_eq!(calls[0]["provider"], "gemini");
    assert_eq!(calls[0]["input_tokens"], 100);
    assert_eq!(calls[0]["cost_usd"], Value::Null);
    assert_eq!(calls[1]["provider"], "anthropic");
}

#[tokio::test]
async fn fallback_models_are_tried_in_order_on_unavailability() {
    let (context, llm) = setup().await;
    configure(
        &context,
        &llm,
        json!({ "redator.fallback_models": ["modelo-b", "modelo-c"] }),
    )
    .await;
    let id = create(&context, TEXT).await;
    llm.push(503, json!({ "error": "indisponivel" }))
        .push(429, json!({ "error": "limite" }))
        .push(
            200,
            openai(json!({ "title": "Pelo terceiro", "text": TEXT })),
        );
    run(&context).await;
    assert_eq!(memory(&context, &id).await["title"], "Pelo terceiro");
    let models: Vec<Value> = llm
        .received()
        .iter()
        .map(|request| request.body["model"].clone())
        .collect();
    assert_eq!(
        models,
        vec![
            json!("modelo-principal"),
            json!("modelo-b"),
            json!("modelo-c")
        ]
    );
    let outcomes: Vec<Value> = llm_calls(&context, "")
        .await
        .iter()
        .rev()
        .map(|call| call["outcome"].clone())
        .collect();
    assert_eq!(
        outcomes,
        vec![
            json!("unavailable"),
            json!("rate_limited"),
            json!("success")
        ]
    );
}

#[tokio::test]
async fn failures_retry_with_backoff_then_fall_back_and_retry_job_requeues() {
    let (context, llm) = setup().await;
    configure(&context, &llm, json!({ "redator.max_attempts": 2 })).await;
    let id = create(&context, TEXT).await;
    llm.push(500, json!({}));
    run(&context).await;
    let job = redact_jobs(&context, &id).await.remove(0);
    assert_eq!(job["state"], "queued");
    assert_eq!(job["attempts"], 1);
    assert_eq!(job["last_error"], "provedor indisponivel");
    assert_eq!(memory(&context, &id).await["title_state"], "pending");
    // Backoff de 2 s: ainda não está pronto agora.
    assert_eq!(run(&context).await, 0);

    // Resposta inválida conta como falha da tentativa; a segunda esgota as tentativas.
    llm.push(
        200,
        openai(json!({ "title": "x".repeat(150), "text": TEXT })),
    );
    let later = Utc::now() + chrono::Duration::seconds(3);
    assert_eq!(context.core.run_redact_jobs(later).await, 1);
    let job = redact_jobs(&context, &id).await.remove(0);
    assert_eq!(job["state"], "failed");
    assert_eq!(job["attempts"], 2);
    assert!(
        job["last_error"]
            .as_str()
            .expect("error")
            .contains("title_max")
    );
    assert_eq!(memory(&context, &id).await["title_state"], "fallback");
    assert_eq!(jobs(&context, "?state=failed").await.len(), 1);
    let calls = llm_calls(&context, "").await;
    assert_eq!(calls[0]["outcome"], "invalid_response");

    let job_id = job["id"].as_i64().expect("id");
    let (status, retried) = context
        .call(
            "POST",
            &format!("/v1/jobs/{job_id}/retry"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{retried}");
    assert_eq!(retried["state"], "queued");
    assert_eq!(retried["attempts"], 0);
    let (status, again) = context
        .call(
            "POST",
            &format!("/v1/jobs/{job_id}/retry"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(again["error"]["code"], "JOB_NOT_RETRYABLE");
    let (status, missing) = context
        .call("POST", "/v1/jobs/999999/retry", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["code"], "JOB_NOT_FOUND");
    let (status, invalid) = context
        .call("POST", "/v1/jobs/abc/retry", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid}");

    // A memória em fallback ganha o título quando o job volta a rodar.
    llm.push(200, openai(json!({ "title": "Enfim", "text": TEXT })));
    run(&context).await;
    let after = memory(&context, &id).await;
    assert_eq!(after["title"], "Enfim");
    assert_eq!(after["title_state"], "ready");
    assert_eq!(
        context.count("SELECT COUNT(*) FROM audit_log WHERE action = 'job.retry'"),
        1
    );
}

#[tokio::test]
async fn a_timeout_counts_as_a_failed_attempt() {
    let (context, llm) = setup().await;
    configure(
        &context,
        &llm,
        json!({ "redator.timeout_ms": 1000, "redator.max_attempts": 1 }),
    )
    .await;
    let id = create(&context, TEXT).await;
    llm.push_delayed(
        200,
        openai(json!({ "title": "Tarde demais", "text": TEXT })),
        Duration::from_millis(1500),
    );
    run(&context).await;
    let job = redact_jobs(&context, &id).await.remove(0);
    assert_eq!(job["state"], "failed");
    assert_eq!(job["last_error"], "tempo limite");
    assert_eq!(memory(&context, &id).await["title_state"], "fallback");
    assert_eq!(llm_calls(&context, "").await[0]["outcome"], "timeout");
}

#[tokio::test]
async fn a_disabled_or_unconfigured_redator_falls_back_without_calling_anything() {
    let (context, llm) = setup().await;
    let unconfigured = create(&context, TEXT).await;
    run(&context).await;
    assert_eq!(
        memory(&context, &unconfigured).await["title_state"],
        "fallback"
    );
    assert_eq!(
        redact_jobs(&context, &unconfigured).await[0]["outcome"],
        "skipped"
    );

    configure(&context, &llm, json!({ "redator.enabled": false })).await;
    let disabled = create(&context, "Deploy acontece no merge para a main").await;
    run(&context).await;
    assert_eq!(memory(&context, &disabled).await["title_state"], "fallback");
    assert!(llm.received().is_empty());
    assert!(llm_calls(&context, "").await.is_empty());
}

#[tokio::test]
async fn jobs_of_a_superseded_content_end_without_effect() {
    let (context, llm) = setup().await;
    configure(&context, &llm, json!({})).await;
    let id = create(&context, TEXT).await;
    let (status, body) = context
        .call(
            "PATCH",
            &format!("/v1/memories/{id}"),
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "Use cargo nextest com o filtro do modulo" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    llm.push(
        200,
        openai(json!({ "title": "Novo texto", "text": "Texto revisado." })),
    );
    assert_eq!(run(&context).await, 2);
    let jobs = redact_jobs(&context, &id).await;
    assert_eq!(jobs[0]["outcome"], "title_only");
    assert_eq!(jobs[1]["outcome"], "superseded");
    assert_eq!(llm.received().len(), 1, "o job superado nao chama o LLM");
    assert_eq!(memory(&context, &id).await["title"], "Novo texto");

    // Fixar a memória não supera o job (só o conteúdo supera).
    let pinned = create(&context, "Segredos ficam cifrados com a SECRETS_KEY").await;
    let (status, _) = context
        .call(
            "POST",
            &format!("/v1/memories/{pinned}/pin"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    llm.push(
        200,
        openai(json!({ "title": "Segredos cifrados", "text": "Texto revisado." })),
    );
    run(&context).await;
    assert_eq!(
        memory(&context, &pinned).await["title"],
        "Segredos cifrados"
    );

    let deleted = create(&context, "Memoria que sera excluida antes do Redator").await;
    let (status, _) = context
        .call(
            "DELETE",
            &format!("/v1/memories/{deleted}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    run(&context).await;
    assert_eq!(
        redact_jobs(&context, &deleted).await[0]["outcome"],
        "superseded"
    );
    assert_eq!(llm.received().len(), 2);
}

#[tokio::test]
async fn an_expired_lease_returns_the_job_to_the_queue() {
    let (context, llm) = setup().await;
    configure(&context, &llm, json!({})).await;
    let id = create(&context, TEXT).await;
    let now = Utc::now();
    let stamp = |at: chrono::DateTime<Utc>| at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    use engramind::store::Store as _;
    let claimed = context
        .store
        .claim_job(
            "redact".to_owned(),
            stamp(now),
            stamp(now + chrono::Duration::seconds(30)),
        )
        .await
        .expect("claim")
        .expect("job");
    assert_eq!(claimed.attempts, 1);
    // Com o lease vivo, ninguém mais pega o job.
    assert_eq!(run(&context).await, 0);
    llm.push(
        200,
        openai(json!({ "title": "Depois do lease", "text": TEXT })),
    );
    let later = now + chrono::Duration::seconds(31);
    assert_eq!(context.core.run_redact_jobs(later).await, 1);
    assert_eq!(memory(&context, &id).await["title"], "Depois do lease");
    let job = redact_jobs(&context, &id).await.remove(0);
    assert_eq!(job["attempts"], 2);
    // O executor antigo não consegue mais gravar o fim.
    let finished = context
        .store
        .finish_job(
            claimed.id,
            engramind::core::model::JobFinish::Fail {
                error: "atrasado".to_owned(),
            },
            stamp(later),
        )
        .await
        .expect("finish");
    assert!(!finished);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_background_workers_redact_right_after_the_write() {
    let (context, llm) = setup().await;
    configure(&context, &llm, json!({})).await;
    let workers = context.core.spawn_workers();
    llm.push(
        200,
        openai(json!({ "title": "Em segundo plano", "text": TEXT })),
    );
    let id = create(&context, TEXT).await;
    let mut title_state = Value::Null;
    for _ in 0..50 {
        title_state = memory(&context, &id).await["title_state"].clone();
        if title_state == "ready" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(title_state, "ready");
    for worker in workers {
        worker.abort();
    }
}

#[tokio::test]
async fn secrets_are_sealed_listed_without_value_and_never_settings() {
    let (context, _llm) = setup().await;
    let (status, body) = set_key(&context, json!(API_KEY)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["configured"], true);
    assert_eq!(body["key"], "redator.api_key");
    assert!(!body.to_string().contains(API_KEY));
    let stored: Vec<u8> = context
        .db()
        .query_row("SELECT value_enc FROM secrets", [], |row| row.get(0))
        .expect("secret");
    assert!(stored.starts_with(b"v1"));
    assert!(
        !stored
            .windows(API_KEY.len())
            .any(|window| window == API_KEY.as_bytes())
    );

    let (_, settings) = context
        .call(
            "GET",
            "/v1/settings?key=redator.api_key",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    let secret = &settings["settings"][0];
    assert_eq!(secret["secret"], true);
    assert_eq!(secret["configured"], true);
    assert_eq!(secret["value"], Value::Null);
    let (_, all) = context
        .call("GET", "/v1/settings", Some(ADMIN_TOKEN), None)
        .await;
    assert!(!all.to_string().contains(API_KEY));

    let (status, refused) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "redator.api_key": API_KEY })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refused["error"]["code"], "SETTING_IS_SECRET");
    let (status, unknown) = context
        .call(
            "PUT",
            "/v1/secrets/redator.model",
            Some(ADMIN_TOKEN),
            Some(json!({ "value": "x" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(unknown["error"]["code"], "SECRET_UNKNOWN");
    let (status, empty) = context
        .call(
            "PUT",
            "/v1/secrets/redator.api_key",
            Some(ADMIN_TOKEN),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{empty}");

    let (status, deleted) = set_key(&context, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted["configured"], false);
    assert_eq!(context.count("SELECT COUNT(*) FROM secrets"), 0);
    let audit: Vec<String> = {
        let db = context.db();
        let mut statement = db
            .prepare("SELECT COALESCE(before_json, '') || COALESCE(after_json, '') FROM audit_log WHERE entity = 'secret'")
            .expect("prepare");
        statement
            .query_map([], |row| row.get(0))
            .expect("audit")
            .collect::<Result<_, _>>()
            .expect("rows")
    };
    assert_eq!(audit.len(), 2);
    assert!(audit.iter().all(|row| !row.contains(API_KEY)));
    let idempotency: i64 = context.count(&format!(
        "SELECT COUNT(*) FROM idempotency WHERE response_json LIKE '%{API_KEY}%'"
    ));
    assert_eq!(idempotency, 0);
}

#[tokio::test]
async fn test_redator_uses_the_proposal_without_saving_it() {
    let (context, llm) = setup().await;
    let body = |settings: Value| {
        json!({
            "text": "Use cargo nextest localmente",
            "settings": settings,
        })
    };
    let (status, missing) = context
        .call(
            "POST",
            "/v1/settings/redator/test",
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "Use cargo nextest" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(missing["error"]["code"], "PROVIDER_NOT_CONFIGURED");
    assert_eq!(
        missing["error"]["details"]["missing"],
        json!(["redator.provider", "redator.model"])
    );

    let proposal = json!({
        "redator.provider": "anthropic",
        "redator.model": "proposto",
        "redator.base_url": llm.url,
        "redator.api_key": API_KEY,
    });
    llm.push(
        200,
        anthropic(json!({ "title": "Teste", "text": "Rode cargo nextest localmente." })),
    );
    let (status, _, result) = context
        .call_with(
            "POST",
            "/v1/settings/redator/test",
            Some(ADMIN_TOKEN),
            Some(body(proposal.clone())),
            &[("Idempotency-Key", "teste-1")],
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["title"], "Teste");
    assert_eq!(result["text"], "Rode cargo nextest localmente.");
    assert_eq!(result["model"], "proposto");
    assert!(result["latency_ms"].as_i64().is_some());
    assert_eq!(llm.received()[0].headers["x-api-key"], API_KEY);
    // Nada foi gravado: nem configuração nem segredo.
    let (_, settings) = context
        .call(
            "GET",
            "/v1/settings?key=redator.model",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(settings["settings"][0]["value"], Value::Null);
    assert_eq!(context.count("SELECT COUNT(*) FROM secrets"), 0);
    let calls = llm_calls(&context, "?purpose=test").await;
    assert_eq!(calls.len(), 1);
    // A mesma chave repete a resposta sem chamar o LLM de novo.
    let (status, _, replay) = context
        .call_with(
            "POST",
            "/v1/settings/redator/test",
            Some(ADMIN_TOKEN),
            Some(body(proposal.clone())),
            &[("Idempotency-Key", "teste-1")],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["title"], "Teste");
    assert_eq!(llm.received().len(), 1);
    assert_eq!(
        context.count(&format!(
            "SELECT COUNT(*) FROM idempotency WHERE response_json LIKE '%{API_KEY}%'"
        )),
        0
    );

    llm.push(401, json!({ "error": { "message": "chave invalida" } }));
    let (status, failed) = context
        .call(
            "POST",
            "/v1/settings/redator/test",
            Some(ADMIN_TOKEN),
            Some(body(proposal.clone())),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(failed["error"]["code"], "PROVIDER_REJECTED");
    assert_eq!(failed["error"]["details"]["outcome"], "rejected");
    assert_eq!(
        failed["error"]["details"]["reason"],
        "recusado pelo provedor (HTTP 401: chave invalida)"
    );
    assert!(!failed.to_string().contains(API_KEY));

    let mut slow = proposal.clone();
    slow["redator.timeout_ms"] = json!(1000);
    llm.push_delayed(
        200,
        anthropic(json!({ "title": "x", "text": "y" })),
        Duration::from_millis(1500),
    );
    let (status, timeout) = context
        .call(
            "POST",
            "/v1/settings/redator/test",
            Some(ADMIN_TOKEN),
            Some(body(slow)),
        )
        .await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(timeout["error"]["code"], "PROVIDER_TIMEOUT");
    assert_eq!(llm_calls(&context, "?purpose=test").await.len(), 3);

    let (status, unknown) = context
        .call(
            "POST",
            "/v1/settings/redator/test",
            Some(ADMIN_TOKEN),
            Some(body(json!({ "memory.text_max": 10 }))),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(unknown["error"]["code"], "SETTING_UNKNOWN");
}

#[tokio::test]
async fn status_shows_the_queue_and_the_redator() {
    let (context, llm) = setup().await;
    create(&context, TEXT).await;
    let (status, body) = context
        .call("GET", "/v1/status", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["queue"]["pending"], 2, "redact e embed");
    assert_eq!(body["redator"]["configured"], false);
    assert_eq!(
        body["redator"]["missing"],
        json!(["redator.provider", "redator.model"])
    );
    configure(&context, &llm, json!({})).await;
    llm.push(200, openai(json!({ "title": "T", "text": TEXT })));
    run(&context).await;
    let (_, body) = context
        .call("GET", "/v1/status", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(body["redator"]["configured"], true);
    assert_eq!(body["redator"]["provider"], "openai_compatible");
    assert_eq!(body["redator"]["model"], "modelo-principal");
    assert!(body["redator"]["activity"]["last_success_at"].is_string());
    // O título novo reenfileira o `embed` (o vetor é de título + texto).
    assert_eq!(
        body["queue"]["pending"], 2,
        "o embed da escrita e o do titulo"
    );
    assert!(!body.to_string().contains(API_KEY));
}

#[tokio::test]
async fn mcp_and_cli_set_the_secret_without_echoing_it() {
    let (context, _llm) = setup().await;
    let response = context
        .call_tool(
            ADMIN_TOKEN,
            "set_secret",
            json!({ "key": "redator.api_key", "value": API_KEY }),
        )
        .await;
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(response["result"]["structuredContent"]["configured"], true);
    assert!(!response.to_string().contains(API_KEY));

    let url = context.serve().await;
    let home = context.dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let bin = std::env::var("NEXTEST_BIN_EXE_engramind")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_engramind").to_owned());
    let output = tokio::task::spawn_blocking(move || {
        use std::io::Write as _;
        let mut child = std::process::Command::new(bin)
            .args(["secrets", "set", "redator.api_key", "--json"])
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &home)
            .env("ENGRAMIND_URL", url)
            .env("ENGRAMIND_TOKEN", ADMIN_TOKEN)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(b"sk-pela-entrada-padrao\n")
            .expect("write");
        child.wait_with_output().expect("output")
    })
    .await
    .expect("join");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("sk-pela-entrada-padrao"));
    let body: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(body["configured"], true);
    let opened = context
        .db()
        .query_row("SELECT length(value_enc) FROM secrets", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("secret");
    assert_eq!(opened, 2 + 12 + "sk-pela-entrada-padrao".len() as i64 + 16);
}

/// Corpo enviado ao provedor em `test_redator` com a proposta e a temperatura informada.
async fn sent_body(
    context: &TestContext,
    llm: &ScriptedLlm,
    provider: &str,
    reply: Value,
    temperature: Value,
) -> Value {
    llm.push(200, reply);
    let base_url = if provider == "openai_compatible" {
        format!("{}/v1", llm.url)
    } else {
        llm.url.clone()
    };
    let (status, body) = context
        .call(
            "POST",
            "/v1/settings/redator/test",
            Some(ADMIN_TOKEN),
            Some(
                json!({ "text": "Use cargo nextest localmente", "settings": {
                "redator.provider": provider,
                "redator.model": "claude-haiku-5-5",
                "redator.base_url": base_url,
                "redator.api_key": API_KEY,
                "redator.temperature": temperature,
            } }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{provider}: {body}");
    llm.received().pop().expect("request").body
}

#[tokio::test]
async fn temperature_is_sent_only_when_configured() {
    let (context, llm) = setup().await;
    let output = json!({ "title": "Teste", "text": "Use cargo nextest localmente" });
    for (provider, reply) in [
        ("openai_compatible", openai(output.clone())),
        ("anthropic", anthropic(output.clone())),
        ("gemini", gemini(output.clone())),
    ] {
        let body = sent_body(&context, &llm, provider, reply.clone(), Value::Null).await;
        assert!(body.get("temperature").is_none(), "{provider}: {body}");
        assert!(
            body["generationConfig"].get("temperature").is_none(),
            "{provider}: {body}"
        );
        assert!(
            !body.to_string().contains("temperature"),
            "{provider}: {body}"
        );

        let body = sent_body(&context, &llm, provider, reply, json!(0.3)).await;
        let sent = if provider == "gemini" {
            &body["generationConfig"]["temperature"]
        } else {
            &body["temperature"]
        };
        assert_eq!(sent, &json!(0.3), "{provider}: {body}");
    }

    // A chave gravada vale para o job `redact`; `null` volta a omitir o campo.
    configure(&context, &llm, json!({ "redator.temperature": 1.5 })).await;
    llm.push(200, openai(json!({ "title": "Testes", "text": TEXT })));
    create(&context, TEXT).await;
    run(&context).await;
    assert_eq!(
        llm.received().pop().expect("redact").body["temperature"],
        json!(1.5)
    );
}

#[tokio::test]
async fn temperature_out_of_range_is_refused_and_nothing_is_saved() {
    let context = TestContext::new().await;
    for value in [json!(2.5), json!(-0.1), json!("0.3")] {
        let (status, body) = context
            .call(
                "PUT",
                "/v1/settings",
                Some(ADMIN_TOKEN),
                Some(json!({ "redator.model": "m", "redator.temperature": value })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "SETTING_INVALID");
        assert_eq!(body["error"]["details"]["key"], "redator.temperature");
    }
    let (_, settings) = context
        .call(
            "GET",
            "/v1/settings?key=redator.model",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(settings["settings"][0]["value"], Value::Null);
    put_settings(&context, json!({ "redator.temperature": 0.3 })).await;
    put_settings(&context, json!({ "redator.temperature": null })).await;
}

#[tokio::test]
async fn a_provider_rejection_keeps_its_reason_without_the_key() {
    let (context, llm) = setup().await;
    let reason = "`temperature` is deprecated for this model.";
    llm.push(
        400,
        json!({ "type": "error", "error": { "type": "invalid_request_error", "message": reason } }),
    );
    let (status, failed) = context
        .call(
            "POST",
            "/v1/settings/redator/test",
            Some(ADMIN_TOKEN),
            Some(
                json!({ "text": "Use cargo nextest localmente", "settings": {
                "redator.provider": "anthropic",
                "redator.model": "claude-haiku-5-5",
                "redator.base_url": llm.url,
                "redator.api_key": API_KEY,
                "redator.temperature": 0,
            } }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{failed}");
    let expected = format!("recusado pelo provedor (HTTP 400: {reason} (invalid_request_error))");
    assert_eq!(failed["error"]["code"], "PROVIDER_REJECTED");
    assert_eq!(failed["error"]["details"]["outcome"], "rejected");
    assert_eq!(failed["error"]["details"]["reason"], expected);
    assert!(
        failed["error"]["message"]
            .as_str()
            .expect("message")
            .contains(reason)
    );
    let calls = llm_calls(&context, "?purpose=test").await;
    assert_eq!(calls[0]["outcome"], "rejected");
    assert_eq!(calls[0]["error"], expected);

    // Uma chave ecoada pelo provedor some; o motivo fica curto.
    llm.push(
        401,
        json!({ "error": { "message": format!("Incorrect API key provided: {API_KEY}. {}", "x".repeat(400)) } }),
    );
    let (status, failed) = context
        .call(
            "POST",
            "/v1/settings/redator/test",
            Some(ADMIN_TOKEN),
            Some(
                json!({ "text": "Use cargo nextest localmente", "settings": {
                "redator.provider": "anthropic",
                "redator.model": "claude-haiku-5-5",
                "redator.base_url": llm.url,
                "redator.api_key": API_KEY,
            } }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{failed}");
    assert!(!failed.to_string().contains(API_KEY), "{failed}");
    let reason = failed["error"]["details"]["reason"]
        .as_str()
        .expect("reason");
    assert!(reason.contains("[REDACTED]"), "{reason}");
    assert!(reason.chars().count() <= 200, "{reason}");
    let calls = llm_calls(&context, "?purpose=test").await;
    assert!(calls.iter().all(|call| !call.to_string().contains(API_KEY)));
    assert!(calls.iter().all(|call| {
        call["error"]
            .as_str()
            .is_none_or(|error| error.chars().count() <= 200)
    }));
}
