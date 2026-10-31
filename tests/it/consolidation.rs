//! Consolidação (009): duplicatas, importância, desuso, `run_consolidation` (com `dry_run`),
//! agendador e `mark_verified`, contra o LLM roteirizado e os embeddings por hash.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::{TimeZone, Utc};
use engramind::core::redator::guard::{GuardFuture, MeaningGuard};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;

use crate::support::{
    ADMIN_TOKEN, TestContext,
    llm::{ScriptedLlm, openai},
};

const CHANNEL: &str = "eng/global";
const A: &str = "Rode os testes afetados com cargo nextest antes de abrir o pull request";
const B: &str = "Rode os testes afetados com cargo nextest antes de abrir cada pull request";
const C: &str = "A porta padrao do ambiente de preview fica registrada no arquivo de portas";
const MERGED: &str = "Rode os testes afetados com cargo nextest antes de abrir cada pull request.";
const OLD: &str = "2026-01-01T00:00:00.000Z";

struct FixedGuard(f64);

impl MeaningGuard for FixedGuard {
    fn similarity<'a>(&'a self, _original: &'a str, _revised: &'a str) -> GuardFuture<'a> {
        let similarity = self.0;
        Box::pin(async move { Some(similarity) })
    }
}

async fn ok(context: &TestContext, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, response) = context.call(method, uri, Some(ADMIN_TOKEN), body).await;
    assert!(status.is_success(), "{method} {uri}: {status} {response}");
    response
}

/// Canal com a consolidação ligada, Redator apontado para o LLM roteirizado e guarda fixa.
async fn setup() -> (TestContext, ScriptedLlm) {
    let context = TestContext::new().await;
    context.create_channel(CHANNEL).await;
    let llm = ScriptedLlm::start().await;
    ok(
        &context,
        "PUT",
        "/v1/settings",
        Some(json!({
            "redator.provider": "openai_compatible",
            "redator.model": "modelo-principal",
            "redator.base_url": format!("{}/v1", llm.url),
            "redator.timeout_ms": 2000,
            "consolidation.duplicate_threshold": 0.8,
        })),
    )
    .await;
    ok(
        &context,
        "PUT",
        "/v1/secrets/redator.api_key",
        Some(json!({ "value": "sk-segredo-do-provedor-de-teste" })),
    )
    .await;
    enable(&context, CHANNEL).await;
    context.core.set_meaning_guard(Arc::new(FixedGuard(0.95)));
    (context, llm)
}

async fn enable(context: &TestContext, channel: &str) {
    ok(
        context,
        "PUT",
        &format!("/v1/channels/{}/settings", channel.replace('/', "%2F")),
        Some(json!({ "consolidation.enabled": true })),
    )
    .await;
}

async fn create(context: &TestContext, text: &str) -> String {
    let body = context.create_memory(ADMIN_TOKEN, CHANNEL, text).await;
    body["memory"]["id"].as_str().expect("id").to_owned()
}

async fn memory(context: &TestContext, id: &str) -> Value {
    ok(context, "GET", &format!("/v1/memories/{id}"), None).await
}

async fn run(context: &TestContext, body: Value) -> (StatusCode, Value) {
    context
        .call(
            "POST",
            "/v1/consolidation/run",
            Some(ADMIN_TOKEN),
            Some(body),
        )
        .await
}

async fn embed(context: &TestContext) {
    context.core.run_embed_jobs(Utc::now()).await;
}

async fn consolidate(context: &TestContext) -> usize {
    context.core.run_consolidate_jobs(Utc::now()).await
}

fn merge(text: Option<&str>) -> Value {
    openai(json!({ "merge": text.is_some(), "text": text.unwrap_or("") }))
}

fn scores(pairs: &[(&str, f64)]) -> Value {
    openai(json!({
        "scores": pairs
            .iter()
            .map(|(id, importance)| json!({ "id": id, "importance": importance }))
            .collect::<Vec<_>>()
    }))
}

fn audits(context: &TestContext, action: &str) -> Vec<Value> {
    let connection = context.db();
    let mut statement = connection
        .prepare("SELECT after_json FROM audit_log WHERE action = ?1 ORDER BY id")
        .expect("prepare");
    statement
        .query_map([action], |row| row.get::<_, Option<String>>(0))
        .expect("query")
        .map(|row| serde_json::from_str(&row.expect("row").expect("after")).expect("json"))
        .collect()
}

fn age(context: &TestContext, id: &str) {
    context
        .db()
        .execute(
            "UPDATE memories SET created_at = ?1, updated_at = ?1 WHERE id = ?2",
            [OLD, id],
        )
        .expect("age");
}

async fn context_block(app: &axum::Router) -> (String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::get("/v1/context?format=tagged")
                .header(header::AUTHORIZATION, format!("Bearer {ADMIN_TOKEN}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("json");
    (
        body["version"].as_str().expect("version").to_owned(),
        body["content"].as_str().expect("content").to_owned(),
    )
}

#[tokio::test]
async fn a_run_merges_classifies_and_flags_exactly_what_the_dry_run_planned() {
    let (context, llm) = setup().await;
    let a = create(&context, A).await;
    let b = create(&context, B).await;
    let c = create(&context, C).await;
    embed(&context).await;
    age(&context, &c);
    // App admin com webhook recebe os eventos da fusão.
    ok(
        &context,
        "PATCH",
        "/v1/apps/bootstrap",
        Some(json!({
            "webhook_url": "http://127.0.0.1:9/eventos",
            "webhook_secret": "segredo-do-webhook-com-mais-de-32-caracteres",
        })),
    )
    .await;
    let (status, plan) = run(&context, json!({ "channel": CHANNEL, "dry_run": true })).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    let planned = &plan["channels"][0];
    assert_eq!(planned["channel"], CHANNEL);
    assert_eq!(planned["duplicates"].as_array().expect("pairs").len(), 1);
    assert_eq!(planned["duplicates"][0]["keep"], a.as_str());
    assert_eq!(planned["duplicates"][0]["archive"], b.as_str());
    assert!(
        planned["duplicates"][0]["similarity"]
            .as_f64()
            .expect("cos")
            >= 0.8
    );
    assert_eq!(planned["classify"], json!([c]));
    assert_eq!(planned["stale"]["flag"], json!([c]));
    assert_eq!(planned["skipped"], json!({}));
    assert!(llm.received().is_empty(), "dry_run nao chama o LLM");
    assert_eq!(
        context.count("SELECT COUNT(*) FROM jobs WHERE kind = 'consolidate'"),
        0
    );

    llm.push(200, merge(Some(MERGED)));
    llm.push(200, scores(&[(&c, 0.7)]));
    let webhooks_before = context.count("SELECT COUNT(*) FROM jobs WHERE kind = 'webhook'");
    let (status, queued) = run(&context, json!({ "channel": CHANNEL })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{queued}");
    assert_eq!(queued["dry_run"], false);
    assert_eq!(queued["jobs"][0]["already_queued"], false);
    assert_eq!(consolidate(&context).await, 1);

    let kept = memory(&context, &a).await;
    assert_eq!(kept["text"], MERGED);
    assert_eq!(kept["version"], 2);
    assert_eq!(kept["status"], "active");
    assert_eq!(kept["title_state"], "pending");
    let merged = memory(&context, &b).await;
    assert_eq!(merged["status"], "archived");
    assert_eq!(merged["merged_into"], a.as_str());
    let history = ok(&context, "GET", &format!("/v1/memories/{a}/history"), None).await;
    let revision = &history["revisions"][0];
    assert_eq!(revision["reason"], "consolidation.merge");
    assert_eq!(revision["author"]["app_id"], "system");
    assert_eq!(revision["author"]["agent_id"], "consolidador");
    let classified = memory(&context, &c).await;
    assert_eq!(
        classified["importance"],
        Value::Null,
        "nota so vale na proxima mudanca"
    );
    assert_eq!(classified["stale_candidate"], true);
    let pending: f64 = context
        .db()
        .query_row(
            "SELECT importance_pending FROM memories WHERE id = ?1",
            [&c],
            |row| row.get(0),
        )
        .expect("pending");
    assert!((pending - 0.7).abs() < 1e-9);

    let report = audits(&context, "consolidation.run");
    assert_eq!(report.len(), 1);
    assert_eq!(report[0]["trigger"], "manual");
    assert_eq!(report[0]["report"]["merged"], 1);
    assert_eq!(report[0]["report"]["classified"], 1);
    assert_eq!(report[0]["report"]["flagged"], 1);
    assert_eq!(report[0]["report"]["failed"], 0);
    let calls = ok(&context, "GET", "/v1/llm-calls?purpose=consolidation", None).await;
    assert_eq!(calls["calls"].as_array().expect("calls").len(), 2);
    // O texto da memória vai entre as marcas, como dado.
    let received = llm.received();
    let user = received[0].body["messages"][1]["content"]
        .as_str()
        .expect("user");
    assert!(user.contains(A) && user.contains(B));
    // Fusão: um `memory.updated` (a mantida) e um `memory.archived` (a fundida).
    let events: Vec<(String, Value)> = {
        let connection = context.db();
        let mut statement = connection
            .prepare("SELECT payload_json FROM jobs WHERE kind = 'webhook' ORDER BY id")
            .expect("prepare");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .skip(webhooks_before as usize)
            .map(|row| {
                let payload: Value = serde_json::from_str(&row.expect("row")).expect("json");
                (
                    payload["event"]["type"].as_str().expect("type").to_owned(),
                    payload["event"]["memory_ids"].clone(),
                )
            })
            .collect()
    };
    assert_eq!(
        events,
        vec![
            ("memory.updated".to_owned(), json!([a])),
            ("memory.archived".to_owned(), json!([b])),
        ]
    );
    let status = ok(&context, "GET", "/v1/status", None).await;
    let summary = &status["consolidation"];
    assert_eq!(summary["enabled_channels"], 1);
    assert_eq!(summary["schedule"], "0 3 * * *");
    assert_eq!(summary["last_24h"]["merged"], 1);
    assert_eq!(summary["llm_last_24h"]["calls"], 2);
    let stale = ok(&context, "GET", "/v1/memories?stale=true", None).await;
    assert_eq!(stale["memories"][0]["id"], c.as_str());
    assert_eq!(stale["memories"].as_array().expect("list").len(), 1);
    let stats = ok(&context, "GET", "/v1/channels/eng%2Fglobal/stats", None).await;
    assert_eq!(stats["stale"], 1);
    // Restaurar a fundida desfaz o vínculo.
    let restored = ok(&context, "POST", &format!("/v1/memories/{b}/restore"), None).await;
    assert_eq!(restored["merged_into"], Value::Null);
}

#[tokio::test]
async fn importance_only_changes_the_order_with_the_next_real_change_even_after_restart() {
    let (context, llm) = setup().await;
    let first = create(
        &context,
        "Primeira memoria sobre a configuracao do servidor de testes",
    )
    .await;
    let second = create(
        &context,
        "Segunda memoria sobre o deploy em producao do servico",
    )
    .await;
    let (version, content) = context_block(&context.app).await;
    assert!(content.find(&first) < content.find(&second));
    llm.push(200, scores(&[(&first, 0.1), (&second, 0.9)]));
    run(
        &context,
        json!({ "channel": CHANNEL, "tasks": ["importance"] }),
    )
    .await;
    assert_eq!(consolidate(&context).await, 1);
    assert_eq!(
        context_block(&context.app).await,
        (version.clone(), content.clone())
    );
    // Um serviço novo sobre o mesmo banco, sem o cache: mesma versão, mesmo bloco.
    let reopened = context.reopened().await;
    assert_eq!(context_block(&reopened).await, (version.clone(), content));
    // A próxima mudança real do canal promove as notas junto com a versão.
    create(
        &context,
        "Terceira memoria sobre um assunto completamente diferente",
    )
    .await;
    let (changed, content) = context_block(&context.app).await;
    assert_ne!(changed, version);
    assert!(content.find(&second) < content.find(&first), "{content}");
    assert_eq!(memory(&context, &second).await["importance"], 0.9);
    // Editar o texto zera a nota.
    ok(
        &context,
        "PATCH",
        &format!("/v1/memories/{second}"),
        Some(json!({ "text": "Segunda memoria reescrita sobre o deploy do servico" })),
    )
    .await;
    assert_eq!(memory(&context, &second).await["importance"], Value::Null);
}

#[tokio::test]
async fn declined_pairs_permanent_memories_and_disabled_channels_are_left_alone() {
    let (context, llm) = setup().await;
    let a = create(&context, A).await;
    let b = create(&context, B).await;
    embed(&context).await;
    llm.push(200, merge(None));
    run(
        &context,
        json!({ "channel": CHANNEL, "tasks": ["duplicates"] }),
    )
    .await;
    consolidate(&context).await;
    assert_eq!(memory(&context, &b).await["status"], "active");
    assert_eq!(context.count("SELECT COUNT(*) FROM consolidation_pairs"), 1);
    // Recusado: o par não volta ao Redator enquanto os textos forem os mesmos.
    let (_, plan) = run(&context, json!({ "channel": CHANNEL, "dry_run": true })).await;
    assert_eq!(plan["channels"][0]["duplicates"], json!([]));
    run(
        &context,
        json!({ "channel": CHANNEL, "tasks": ["duplicates"] }),
    )
    .await;
    consolidate(&context).await;
    assert_eq!(llm.received().len(), 1);
    // `permanent` nunca entra num par.
    context.core.set_meaning_guard(Arc::new(FixedGuard(0.95)));
    ok(&context, "POST", &format!("/v1/memories/{a}/pin"), None).await;
    ok(
        &context,
        "PATCH",
        &format!("/v1/memories/{b}"),
        Some(json!({ "text": format!("{B} hoje") })),
    )
    .await;
    embed(&context).await;
    let (_, plan) = run(&context, json!({ "channel": CHANNEL, "dry_run": true })).await;
    assert_eq!(plan["channels"][0]["duplicates"], json!([]));
    assert!(
        !plan["channels"][0]["classify"]
            .as_array()
            .expect("classify")
            .contains(&json!(a))
    );
    // Canal desligado: 409 sem dry_run; o dry_run continua permitido.
    context.create_channel("eng/outro").await;
    let (status, error) = run(&context, json!({ "channel": "eng/outro" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "CONSOLIDATION_DISABLED");
    let (status, _) = run(&context, json!({ "channel": "eng/outro", "dry_run": true })).await;
    assert_eq!(status, StatusCode::OK);
    // Sem canal: só os habilitados.
    let (status, queued) = run(&context, json!({})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(queued["jobs"].as_array().expect("jobs").len(), 1);
    assert_eq!(queued["jobs"][0]["channel"], CHANNEL);
    // Já na fila: devolve o mesmo job.
    let (_, again) = run(&context, json!({ "channel": CHANNEL })).await;
    assert_eq!(again["jobs"][0]["already_queued"], true);
    assert_eq!(again["jobs"][0]["job_id"], queued["jobs"][0]["job_id"]);
    // Validação e canais inexistentes ou arquivados.
    let (status, error) = run(&context, json!({ "tasks": [] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (status, _) = run(&context, json!({ "tasks": ["outra"] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = run(&context, json!({ "channel": "nao/existe" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    ok(&context, "POST", "/v1/channels/eng%2Foutro/archive", None).await;
    let (status, error) = run(&context, json!({ "channel": "eng/outro" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "CHANNEL_ARCHIVED");
}

#[tokio::test]
async fn providers_unavailable_skip_tasks_and_failures_do_not_fail_the_job() {
    let (context, llm) = setup().await;
    let a = create(&context, A).await;
    create(&context, B).await;
    create(&context, C).await;
    embed(&context).await;
    // Guarda recusando: o par é recusado e nada muda.
    context.core.set_meaning_guard(Arc::new(FixedGuard(0.1)));
    llm.push(200, merge(Some(MERGED)));
    llm.push(500, json!({ "error": "fora do ar" }));
    run(
        &context,
        json!({ "channel": CHANNEL, "tasks": ["duplicates", "importance"] }),
    )
    .await;
    assert_eq!(consolidate(&context).await, 1);
    assert_eq!(memory(&context, &a).await["text"], A);
    let report = audits(&context, "consolidation.run");
    assert_eq!(report[0]["report"]["declined"], 1);
    assert_eq!(report[0]["report"]["failed"], 1);
    let job = &ok(&context, "GET", "/v1/jobs?kind=consolidate", None).await["jobs"][0];
    assert_eq!(job["state"], "done");
    // Embeddings desligados e Redator desligado: tarefas puladas com o motivo.
    ok(
        &context,
        "PUT",
        "/v1/settings",
        Some(json!({ "embedding.provider": "off" })),
    )
    .await;
    let (_, plan) = run(&context, json!({ "channel": CHANNEL, "dry_run": true })).await;
    assert_eq!(
        plan["channels"][0]["skipped"],
        json!({ "duplicates": "embeddings_off" })
    );
    ok(
        &context,
        "PUT",
        "/v1/settings",
        Some(json!({ "redator.enabled": false })),
    )
    .await;
    let (_, plan) = run(&context, json!({ "channel": CHANNEL, "dry_run": true })).await;
    assert_eq!(
        plan["channels"][0]["skipped"],
        json!({ "duplicates": "redator_not_configured", "importance": "redator_not_configured" })
    );
}

#[tokio::test]
async fn auto_archive_stale_archives_with_a_revision_and_an_event() {
    let (context, _llm) = setup().await;
    let old = create(&context, C).await;
    let recent = create(&context, A).await;
    age(&context, &old);
    ok(
        &context,
        "PUT",
        "/v1/settings",
        Some(json!({ "consolidation.auto_archive_stale": true })),
    )
    .await;
    let version = ok(&context, "GET", "/v1/channels/eng%2Fglobal", None).await["version"].clone();
    run(&context, json!({ "channel": CHANNEL, "tasks": ["stale"] })).await;
    consolidate(&context).await;
    let archived = memory(&context, &old).await;
    assert_eq!(archived["status"], "archived");
    assert_eq!(memory(&context, &recent).await["status"], "active");
    let history = ok(
        &context,
        "GET",
        &format!("/v1/memories/{old}/history"),
        None,
    )
    .await;
    assert_eq!(history["revisions"][0]["reason"], "consolidation.stale");
    let after = ok(&context, "GET", "/v1/channels/eng%2Fglobal", None).await["version"].clone();
    assert_eq!(after.as_i64(), version.as_i64().map(|version| version + 1));
    assert_eq!(audits(&context, "memory.archive")[0]["reason"], "stale");
}

#[tokio::test]
async fn mark_verified_records_the_check_without_touching_the_context() {
    let (context, _llm) = setup().await;
    let id = create(&context, C).await;
    age(&context, &id);
    run(&context, json!({ "channel": CHANNEL, "tasks": ["stale"] })).await;
    consolidate(&context).await;
    assert_eq!(memory(&context, &id).await["stale_candidate"], true);
    let (version, content) = context_block(&context.app).await;
    let before = memory(&context, &id).await;
    let verified = ok(&context, "POST", &format!("/v1/memories/{id}/verify"), None).await;
    assert!(verified["verified_at"].is_string());
    assert_eq!(verified["stale_candidate"], false);
    assert_eq!(verified["version"], before["version"]);
    assert_eq!(verified["updated_at"], before["updated_at"]);
    assert_eq!(context_block(&context.app).await, (version, content));
    assert_eq!(
        context.count("SELECT COUNT(*) FROM memory_revisions WHERE memory_id = (SELECT id FROM memories LIMIT 1)"),
        1
    );
    assert_eq!(audits(&context, "memory.verify").len(), 1);
    // A conferência é um sinal de uso: a próxima execução não sinaliza de novo.
    run(&context, json!({ "channel": CHANNEL, "tasks": ["stale"] })).await;
    consolidate(&context).await;
    assert_eq!(memory(&context, &id).await["stale_candidate"], false);
    let (status, error) = context
        .call(
            "POST",
            "/v1/memories/m_ffffffff/verify",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["error"]["code"], "MEMORY_NOT_FOUND");
}

#[tokio::test]
async fn the_scheduler_fires_once_per_slot_and_survives_restarts() {
    let (context, _llm) = setup().await;
    let at = |day: u32, hour: u32, minute: u32| {
        Utc.with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .single()
            .expect("instante")
    };
    // Primeiro start: só registra a base.
    assert_eq!(
        context
            .core
            .consolidation_tick(at(30, 2, 0))
            .await
            .expect("tick"),
        0
    );
    assert_eq!(
        context
            .core
            .consolidation_tick(at(30, 2, 59))
            .await
            .expect("tick"),
        0
    );
    assert_eq!(
        context
            .core
            .consolidation_tick(at(30, 3, 0))
            .await
            .expect("tick"),
        1
    );
    assert_eq!(
        context
            .core
            .consolidation_tick(at(30, 3, 1))
            .await
            .expect("tick"),
        0
    );
    let jobs = ok(&context, "GET", "/v1/jobs?kind=consolidate", None).await;
    assert_eq!(jobs["jobs"][0]["payload"]["trigger"], "schedule");
    // Dias depois, com o job anterior ainda na fila: dispara uma vez e não duplica.
    assert_eq!(
        context
            .core
            .consolidation_tick(at(31, 12, 0))
            .await
            .expect("tick"),
        0
    );
    assert_eq!(
        context.count("SELECT COUNT(*) FROM jobs WHERE kind = 'consolidate'"),
        1
    );
    let status = ok(&context, "GET", "/v1/status", None).await;
    assert!(
        status["consolidation"]["last_scheduled_at"]
            .as_str()
            .expect("last")
            .starts_with("2026-10-31T12:00")
    );
    assert_eq!(status["consolidation"]["queued"], 1);
    // Canal desligado no meio do caminho: o job termina sem tocar nada.
    ok(
        &context,
        "PUT",
        "/v1/channels/eng%2Fglobal/settings",
        Some(json!({ "consolidation.enabled": false })),
    )
    .await;
    assert_eq!(consolidate(&context).await, 1);
    assert!(audits(&context, "consolidation.run").is_empty());
}

#[tokio::test]
async fn a_merge_over_changed_content_is_superseded() {
    use engramind::{
        core::consolidation::{DeclinedPair, job::MemoryMerge},
        store::{Store as _, StoreError},
    };

    let (context, _llm) = setup().await;
    let a = create(&context, A).await;
    let b = create(&context, B).await;
    let channel_id = memory(&context, &a).await["channel_id"]
        .as_str()
        .expect("channel")
        .to_owned();
    let merge = MemoryMerge {
        channel_id,
        keep_id: a.clone(),
        keep_hash: b"outro conteudo".to_vec(),
        archive_id: b.clone(),
        archive_hash: b"outro conteudo".to_vec(),
        text: None,
        jobs: Vec::new(),
    };
    let result = context
        .store
        .merge_memories(
            merge,
            engramind::core::model::WriteContext {
                actor: engramind::core::model::Actor {
                    app_id: Some("system".to_owned()),
                    agent_id: Some("consolidador".to_owned()),
                    token_id: None,
                },
                at: "2026-10-31T00:00:00.000Z".to_owned(),
                idempotency: None,
                events: Vec::new(),
            },
        )
        .await;
    assert_eq!(result, Err(StoreError::Conflict("MEMORY_CHANGED")));
    assert_eq!(memory(&context, &b).await["status"], "active");
    // Par recusado some quando um dos textos muda.
    context
        .store
        .decline_pair(DeclinedPair {
            memory_a: a.clone().min(b.clone()),
            memory_b: a.clone().max(b.clone()),
            hash_a: Vec::new(),
            hash_b: Vec::new(),
        })
        .await
        .expect("decline");
    run(&context, json!({ "channel": CHANNEL, "tasks": ["stale"] })).await;
    consolidate(&context).await;
    assert_eq!(context.count("SELECT COUNT(*) FROM consolidation_pairs"), 0);
}
