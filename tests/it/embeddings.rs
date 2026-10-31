//! Embeddings e busca híbrida (etapa 007): job `embed`, ramo vetorial com RRF ponderado,
//! dicas, guarda de sentido, troca de modelo com reindexação, `off`, provedores remotos
//! roteirizados, `test_embedding`, `list_embedding_models`, `reembed_all` e `get_status`.

use std::sync::Arc;

use axum::http::StatusCode;
use engramind::providers::embeddings::{EmbedError, HashEmbedder, LocalEmbedder, local::Session};
use serde_json::{Value, json};

use crate::support::{
    ADMIN_TOKEN, TestContext,
    llm::{ScriptedLlm, openai},
};

/// Agente `app-a/curador` com `read,write,delete`, assinando `eng/global` (inject),
/// `eng/busca` (search) e `eng/gestao` (manage).
async fn setup(context: &TestContext) -> String {
    context.create_app("app-a").await;
    context.create_agent("app-a", "curador").await;
    for (slug, mode) in [
        ("eng/global", "inject"),
        ("eng/busca", "search"),
        ("eng/gestao", "manage"),
    ] {
        context.create_channel(slug).await;
        context.subscribe("app-a/curador", slug, mode, None).await;
    }
    context
        .issue(
            "app-a",
            Some("app-a/curador"),
            "agent",
            &["read", "write", "delete"],
        )
        .await
}

/// `suite` e `testes` caem no mesmo balde: "rodar todos os testes" encontra a suíte.
fn embedder() -> Arc<HashEmbedder> {
    Arc::new(HashEmbedder::new().concept(&["testes", "suite"]))
}

async fn memory(context: &TestContext, token: &str, channel: &str, text: &str) -> String {
    let body = context.create_memory(token, channel, text).await;
    body["memory"]["id"].as_str().expect("id").to_owned()
}

async fn run(context: &TestContext) -> usize {
    context.core.run_embed_jobs(chrono::Utc::now()).await
}

async fn search(context: &TestContext, token: &str, body: Value) -> Value {
    let (status, results) = context
        .call("POST", "/v1/memories/search", Some(token), Some(body))
        .await;
    assert_eq!(status, StatusCode::OK, "{results}");
    results
}

fn ids(results: &Value) -> Vec<String> {
    results["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|hit| hit["memory"]["id"].as_str().expect("id").to_owned())
        .collect()
}

async fn hints(context: &TestContext, token: &str, body: Value) -> Vec<String> {
    let (status, result) = context
        .call("POST", "/v1/hints", Some(token), Some(body))
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    result["hints"]
        .as_array()
        .expect("hints")
        .iter()
        .map(|hint| hint["id"].as_str().expect("id").to_owned())
        .collect()
}

async fn set(context: &TestContext, settings: Value) -> (StatusCode, Value) {
    context
        .call("PUT", "/v1/settings", Some(ADMIN_TOKEN), Some(settings))
        .await
}

async fn status(context: &TestContext) -> Value {
    let (status, body) = context
        .call("GET", "/v1/status", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["embedding"].clone()
}

fn vector_model(context: &TestContext, id: &str) -> Option<(String, i64)> {
    context
        .db()
        .query_row(
            "SELECT model_id, dims FROM memory_vectors WHERE memory_id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()
}

fn embedding_state(context: &TestContext, id: &str) -> String {
    context
        .db()
        .query_row(
            "SELECT embedding_state FROM memories WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("state")
}

#[tokio::test]
async fn backlog_is_embedded_and_search_fuses_meaning_and_words() {
    let context = TestContext::new().await;
    context.core.set_local_embedder(embedder());
    let token = setup(&context).await;
    let suite = memory(&context, &token, "eng/busca", "Suite completa com nextest").await;
    let local = memory(&context, &token, "eng/busca", "Rodar o servidor local").await;
    memory(
        &context,
        &token,
        "eng/busca",
        "Deploy na main faz SCP da imagem",
    )
    .await;
    assert_eq!(embedding_state(&context, &suite), "pending");
    // Antes dos vetores, a consulta é vetorizada mas o ramo vetorial não acha nada.
    let before = search(
        &context,
        &token,
        json!({ "query": "rodar todos os testes" }),
    )
    .await;
    assert_eq!(before["mode"], "hybrid");
    assert_eq!(ids(&before), vec![local.clone()]);
    assert_eq!(run(&context).await, 3);
    assert_eq!(embedding_state(&context, &suite), "ready");
    assert_eq!(
        vector_model(&context, &suite),
        Some(("local:multilingual-e5-small".to_owned(), 384))
    );
    let after = search(
        &context,
        &token,
        json!({ "query": "rodar todos os testes" }),
    )
    .await;
    assert_eq!(after["mode"], "hybrid");
    let found = ids(&after);
    assert!(found.contains(&suite), "a suite vem pelo sentido: {after}");
    assert_eq!(
        found[0], local,
        "nos dois ramos, o servidor local vem primeiro"
    );
    let score = after["results"][0]["score"].as_f64().expect("score");
    assert!(score > 1.0 / 61.0, "soma dos dois ramos: {score}");
    // Peso vetorial zero: a suite, só vetorial, cai para o fim com score 0.
    let (code, body) = set(&context, json!({ "search.vector_weight": 0 })).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let text_only = search(
        &context,
        &token,
        json!({ "query": "rodar todos os testes" }),
    )
    .await;
    assert_eq!(ids(&text_only)[0], local);
    assert_eq!(
        text_only["results"]
            .as_array()
            .expect("r")
            .last()
            .expect("last")["score"],
        0.0
    );
    // `search.default_limit` vale de fato.
    set(
        &context,
        json!({ "search.vector_weight": null, "search.default_limit": 1 }),
    )
    .await;
    let limited = search(
        &context,
        &token,
        json!({ "query": "rodar todos os testes" }),
    )
    .await;
    assert_eq!(ids(&limited).len(), 1);
}

#[tokio::test]
async fn provider_failure_keeps_text_search_and_empty_hints() {
    let context = TestContext::new().await;
    let embedder = embedder();
    context.core.set_local_embedder(embedder.clone());
    let token = setup(&context).await;
    let id = memory(
        &context,
        &token,
        "eng/busca",
        "Rodar todos os testes com nextest",
    )
    .await;
    run(&context).await;
    embedder.fail(Some(EmbedError::Timeout));
    let results = search(&context, &token, json!({ "query": "testes" })).await;
    assert_eq!(results["mode"], "text");
    assert_eq!(ids(&results), vec![id]);
    assert!(
        hints(&context, &token, json!({ "text": "rodar todos os testes" }))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn hints_respect_threshold_modes_and_context() {
    let context = TestContext::new().await;
    context.core.set_local_embedder(embedder());
    let token = setup(&context).await;
    let exact = memory(&context, &token, "eng/busca", "rodar todos os testes").await;
    let near = memory(&context, &token, "eng/busca", "Suite completa com nextest").await;
    memory(
        &context,
        &token,
        "eng/gestao",
        "rodar todos os testes no gestor",
    )
    .await;
    let in_context = memory(
        &context,
        &token,
        "eng/global",
        "rodar todos os testes sempre",
    )
    .await;
    run(&context).await;
    set(&context, json!({ "search.hint_threshold": 0.9 })).await;
    let found = hints(&context, &token, json!({ "text": "rodar todos os testes" })).await;
    assert_eq!(
        found,
        vec![exact.clone()],
        "limiar alto e sem o que esta no contexto"
    );
    set(&context, json!({ "search.hint_threshold": 0.1 })).await;
    let found = hints(
        &context,
        &token,
        json!({ "text": "rodar todos os testes", "limit": 10 }),
    )
    .await;
    assert!(found.contains(&exact) && found.contains(&near), "{found:?}");
    assert!(!found.contains(&in_context), "o contexto fica de fora");
    // `manage` nunca entra, nem citado.
    let cited = hints(
        &context,
        &token,
        json!({ "text": "rodar todos os testes", "channels": ["eng/gestao"] }),
    )
    .await;
    assert!(cited.is_empty(), "{cited:?}");
    let (_, result) = context
        .call(
            "POST",
            "/v1/hints",
            Some(&token),
            Some(json!({ "text": "rodar testes" })),
        )
        .await;
    assert_eq!(
        result["hints"][0]
            .as_object()
            .expect("hint")
            .keys()
            .collect::<Vec<_>>(),
        ["channel", "id", "title"]
    );
    let (code, _) = context
        .call(
            "POST",
            "/v1/hints",
            Some(&token),
            Some(json!({ "text": " " })),
        )
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    context.core.flush_counters().await.expect("flush");
    context.core.flush_usage().await.expect("flush");
    assert!(context.count("SELECT COUNT(*) FROM usage_log WHERE op = 'get_hints'") >= 1);
    assert!(
        context.count(&format!(
            "SELECT retrieval_count FROM memories WHERE id = '{exact}'"
        )) >= 1
    );
}

#[tokio::test]
async fn off_skips_vectors_and_turning_on_embeds_only_what_is_missing() {
    let context = TestContext::new().await;
    let embedder = embedder();
    context.core.set_local_embedder(embedder.clone());
    let token = setup(&context).await;
    let first = memory(&context, &token, "eng/busca", "Rodar todos os testes").await;
    run(&context).await;
    let (code, body) = set(&context, json!({ "embedding.provider": "off" })).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(embedding_state(&context, &first), "off");
    let second = memory(&context, &token, "eng/busca", "Deploy na main").await;
    assert_eq!(embedding_state(&context, &second), "off");
    assert_eq!(
        context.count(&format!(
            "SELECT COUNT(*) FROM jobs WHERE kind = 'embed' AND state = 'queued' AND payload_json LIKE '%{second}%'"
        )),
        0
    );
    let results = search(&context, &token, json!({ "query": "testes" })).await;
    assert_eq!(results["mode"], "text");
    assert!(
        hints(&context, &token, json!({ "text": "testes" }))
            .await
            .is_empty()
    );
    assert_eq!(status(&context).await["state"], "off");
    let calls = embedder.calls();
    let (code, body) = set(&context, json!({ "embedding.provider": "local" })).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let embedding = status(&context).await;
    assert_eq!(embedding["state"], "reindexing");
    assert_eq!(embedding["total"], 1, "so a que nao tem vetor: {embedding}");
    run(&context).await;
    assert_eq!(embedder.calls(), calls + 1, "um lote");
    assert_eq!(embedding_state(&context, &second), "ready");
    let embedding = status(&context).await;
    assert_eq!(embedding["state"], "ready");
    assert_eq!(embedding["done"], 2);
}

#[tokio::test]
async fn switching_models_reindexes_without_mixing_dimensions() {
    let context = TestContext::new().await;
    context.core.set_local_embedder(embedder());
    let models = context.dir.path().join("models");
    std::fs::create_dir_all(models.join("models--intfloat--multilingual-e5-small")).expect("e5");
    std::fs::create_dir_all(models.join("models--BAAI--bge-m3")).expect("bge");
    let token = setup(&context).await;
    let ids_: Vec<String> = [
        "Suite completa com nextest",
        "Rodar o servidor local",
        "Deploy na main",
    ]
    .iter()
    .map(|text| text.to_string())
    .collect();
    let mut created = Vec::new();
    for text in &ids_ {
        created.push(memory(&context, &token, "eng/busca", text).await);
    }
    run(&context).await;
    let (code, body) = set(
        &context,
        json!({ "embedding.model": "bge-m3", "embedding.batch_size": 2 }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let embedding = status(&context).await;
    assert_eq!(embedding["state"], "reindexing");
    assert_eq!(embedding["model_id"], "local:bge-m3");
    assert_eq!(embedding["dimensions"], 1024);
    assert_eq!(embedding["total"], 3);
    assert_eq!(embedding["done"], 0);
    assert_eq!(
        context.count("SELECT COUNT(*) FROM jobs WHERE kind = 'embed' AND state = 'queued' AND payload_json LIKE '%memory_ids%'"),
        2,
        "lotes de embedding.batch_size"
    );
    // Durante a reindexação: o modelo antigo não entra, sem erro de dimensão.
    let during = search(
        &context,
        &token,
        json!({ "query": "rodar todos os testes" }),
    )
    .await;
    assert_eq!(during["mode"], "hybrid");
    assert!(!ids(&during).contains(&created[0]), "{during}");
    let (code, body) = context
        .call("POST", "/v1/embeddings/reembed", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "REEMBED_IN_PROGRESS");
    run(&context).await;
    let embedding = status(&context).await;
    assert_eq!(embedding["state"], "ready", "{embedding}");
    assert_eq!(embedding["done"], 3);
    for id in &created {
        assert_eq!(
            vector_model(&context, id),
            Some(("local:bge-m3".to_owned(), 1024))
        );
    }
    assert_eq!(
        context.count("SELECT COUNT(*) FROM memory_vectors WHERE model_id <> 'local:bge-m3'"),
        0
    );
    assert!(
        !models
            .join("models--intfloat--multilingual-e5-small")
            .exists()
    );
    assert!(models.join("models--BAAI--bge-m3").exists());
    let after = search(
        &context,
        &token,
        json!({ "query": "rodar todos os testes" }),
    )
    .await;
    assert!(ids(&after).contains(&created[0]));
    // Modelo local desconhecido é recusado sem gravar.
    let (code, body) = set(&context, json!({ "embedding.model": "gpt-4o" })).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "SETTING_INVALID");
    assert_eq!(status(&context).await["model_id"], "local:bge-m3");
}

#[tokio::test]
async fn reembed_all_reprocesses_everything_and_survives_a_restart() {
    let context = TestContext::new().await;
    context.core.set_local_embedder(embedder());
    let token = setup(&context).await;
    let id = memory(&context, &token, "eng/busca", "Rodar todos os testes").await;
    run(&context).await;
    let (code, _, body) = context
        .call_with(
            "POST",
            "/v1/embeddings/reembed",
            Some(ADMIN_TOKEN),
            None,
            &[("Idempotency-Key", "reembed-1")],
        )
        .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["model_id"], "local:multilingual-e5-small");
    let embedding = status(&context).await;
    assert_eq!(embedding["state"], "reindexing");
    assert_eq!(embedding["total"], 1);
    assert_eq!(embedding_state(&context, &id), "pending");
    // A mesma chave repete a resposta, sem nova reindexação.
    let (_, _, replay) = context
        .call_with(
            "POST",
            "/v1/embeddings/reembed",
            Some(ADMIN_TOKEN),
            None,
            &[("Idempotency-Key", "reembed-1")],
        )
        .await;
    assert_eq!(replay["total"], 1);
    // Reinício com a reindexação pela metade: o estado continua.
    context.core.sync_embeddings().await.expect("sync");
    assert_eq!(status(&context).await["state"], "reindexing");
    run(&context).await;
    assert_eq!(status(&context).await["state"], "ready");
    assert_eq!(embedding_state(&context, &id), "ready");
    // `delete_memory` leva o vetor.
    let (code, _) = context
        .call("DELETE", &format!("/v1/memories/{id}"), Some(&token), None)
        .await;
    assert!(code.is_success());
    assert_eq!(context.count("SELECT COUNT(*) FROM memory_vectors"), 0);
}

#[tokio::test]
async fn deleting_a_memory_mid_reindex_does_not_stall_it() {
    let context = TestContext::new().await;
    context.core.set_local_embedder(embedder());
    let token = setup(&context).await;
    let a = memory(&context, &token, "eng/busca", "Rodar todos os testes").await;
    memory(&context, &token, "eng/busca", "Deploy na main").await;
    run(&context).await;
    let (code, _) = context
        .call("POST", "/v1/embeddings/reembed", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(code, StatusCode::ACCEPTED);
    context
        .call("DELETE", &format!("/v1/memories/{a}"), Some(&token), None)
        .await;
    run(&context).await;
    assert_eq!(status(&context).await["state"], "ready");
}

#[tokio::test]
async fn exhausted_attempts_mark_the_memory_failed_and_the_reindex_failed() {
    let context = TestContext::new().await;
    let embedder = embedder();
    context.core.set_local_embedder(embedder.clone());
    let token = setup(&context).await;
    let id = memory(&context, &token, "eng/busca", "Rodar todos os testes").await;
    run(&context).await;
    let (code, _) = context
        .call("POST", "/v1/embeddings/reembed", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(code, StatusCode::ACCEPTED);
    embedder.fail(Some(EmbedError::Unavailable));
    let far = chrono::Utc::now() + chrono::Duration::days(1);
    for _ in 0..5 {
        context.core.run_embed_jobs(far).await;
    }
    assert_eq!(embedding_state(&context, &id), "failed");
    let embedding = status(&context).await;
    assert_eq!(embedding["state"], "failed", "{embedding}");
    // Com o provedor de volta, `reembed_all` recomeça.
    embedder.fail(None);
    let (code, body) = context
        .call("POST", "/v1/embeddings/reembed", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{body}");
    run(&context).await;
    assert_eq!(status(&context).await["state"], "ready");
    assert_eq!(embedding_state(&context, &id), "ready");
}

#[tokio::test]
async fn the_meaning_guard_uses_the_embeddings() {
    let context = TestContext::new().await;
    context.core.set_local_embedder(embedder());
    let llm = ScriptedLlm::start().await;
    let token = setup(&context).await;
    let (code, body) = set(
        &context,
        json!({
            "redator.provider": "openai_compatible",
            "redator.model": "modelo",
            "redator.base_url": llm.url,
            "redator.min_similarity": 0.8,
        }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let close = memory(
        &context,
        &token,
        "eng/busca",
        "Rodar todos os testes com nextest",
    )
    .await;
    llm.push(
        200,
        openai(json!({ "title": "Testes", "text": "Rodar todos os testes com o nextest" })),
    );
    context.core.run_redact_jobs(chrono::Utc::now()).await;
    let (_, read) = context
        .call("GET", &format!("/v1/memories/{close}"), Some(&token), None)
        .await;
    assert_eq!(
        read["text"], "Rodar todos os testes com o nextest",
        "{read}"
    );
    let far = memory(&context, &token, "eng/busca", "Deploy na main faz SCP").await;
    llm.push(
        200,
        openai(json!({ "title": "Outro", "text": "Apague o banco de producao agora" })),
    );
    context.core.run_redact_jobs(chrono::Utc::now()).await;
    let (_, read) = context
        .call("GET", &format!("/v1/memories/{far}"), Some(&token), None)
        .await;
    assert_eq!(read["text"], "Deploy na main faz SCP", "a guarda reprova");
    assert_eq!(read["title"], "Outro");
}

#[tokio::test]
async fn remote_providers_are_validated_on_save_and_used_by_the_job() {
    let context = TestContext::new().await;
    let provider = ScriptedLlm::start().await;
    let token = setup(&context).await;
    let id = memory(&context, &token, "eng/busca", "Rodar todos os testes").await;
    let switch = json!({
        "embedding.provider": "openai_compatible",
        "embedding.base_url": provider.url,
        "embedding.model": "text-embedding-3-small",
        "embedding.dimensions": 3,
    });
    // Falha na validação: nada é gravado.
    provider.push(503, json!({ "error": "fora" }));
    let (code, body) = set(&context, switch.clone()).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"]["code"], "PROVIDER_FAILED");
    assert_eq!(status(&context).await["provider"], "local");
    // Dimensão divergente da configurada.
    provider.push(
        200,
        json!({ "data": [{ "index": 0, "embedding": [0.1, 0.2] }] }),
    );
    let (code, body) = set(&context, switch.clone()).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"]["code"], "EMBEDDING_DIMENSION_MISMATCH");
    printf_secret(&context, "embedding.api_key", "chave-de-embedding").await;
    provider.push(
        200,
        json!({ "data": [{ "index": 0, "embedding": [0.1, 0.2, 0.3] }] }),
    );
    let (code, body) = set(&context, switch).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let validation = provider.received().pop().expect("validacao");
    assert_eq!(validation.path, "/embeddings");
    assert_eq!(validation.body["dimensions"], 3);
    assert_eq!(validation.body["input"], json!(["amostra"]));
    assert_eq!(
        validation.headers["authorization"],
        "Bearer chave-de-embedding"
    );
    provider.push(
        200,
        json!({ "data": [{ "index": 0, "embedding": [1.0, 0.0, 0.0] }] }),
    );
    run(&context).await;
    let job = provider.received().pop().expect("job");
    assert_eq!(job.body["model"], "text-embedding-3-small");
    let input = job.body["input"][0].as_str().expect("input");
    assert!(
        input.ends_with("Rodar todos os testes"),
        "sem prefixo e5: {input}"
    );
    assert_eq!(
        vector_model(&context, &id),
        Some(("openai_compatible:text-embedding-3-small:3".to_owned(), 3))
    );
    // Falha temporária volta à fila; a busca degrada para texto com o provedor fora.
    let other = memory(&context, &token, "eng/busca", "Deploy na main").await;
    provider.push(429, json!({}));
    run(&context).await;
    assert_eq!(embedding_state(&context, &other), "pending");
    let results = search(&context, &token, json!({ "query": "deploy" })).await;
    assert_eq!(results["mode"], "text", "roteiro vazio responde 500");
    assert!(
        !status(&context)
            .await
            .to_string()
            .contains("chave-de-embedding")
    );
}

async fn printf_secret(context: &TestContext, key: &str, value: &str) {
    let (code, body) = context
        .call(
            "PUT",
            &format!("/v1/secrets/{key}"),
            Some(ADMIN_TOKEN),
            Some(json!({ "value": value })),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn ollama_and_gemini_adapters_speak_their_formats() {
    let context = TestContext::new().await;
    let provider = ScriptedLlm::start().await;
    provider.push(200, json!({ "embeddings": [[0.5, 0.5]] }));
    let (code, body) = context
        .call(
            "POST",
            "/v1/settings/embedding/test",
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "amostra", "settings": {
                "embedding.provider": "ollama",
                "embedding.base_url": provider.url,
                "embedding.model": "nomic-embed-text",
            } })),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["model_id"], "ollama:nomic-embed-text");
    assert_eq!(body["dimensions"], 2);
    let request = provider.received().pop().expect("ollama");
    assert_eq!(request.path, "/api/embed");
    assert_eq!(
        request.body,
        json!({ "model": "nomic-embed-text", "input": ["amostra"] })
    );
    provider.push(
        200,
        json!({ "embeddings": [{ "values": [0.1, 0.2, 0.3] }] }),
    );
    let (code, body) = context
        .call(
            "POST",
            "/v1/settings/embedding/test",
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "amostra", "settings": {
                "embedding.provider": "gemini",
                "embedding.base_url": provider.url,
                "embedding.model": "gemini-embedding-001",
                "embedding.api_key": "chave-gemini",
            } })),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let request = provider.received().pop().expect("gemini");
    assert_eq!(
        request.path,
        "/v1beta/models/gemini-embedding-001:batchEmbedContents"
    );
    assert_eq!(request.headers["x-goog-api-key"], "chave-gemini");
    assert_eq!(
        request.body["requests"][0]["model"],
        "models/gemini-embedding-001"
    );
    // Nada foi gravado pelos testes.
    assert_eq!(status(&context).await["provider"], "local");
    assert_eq!(
        context.count("SELECT COUNT(*) FROM secrets WHERE key = 'embedding.api_key'"),
        0
    );
    // Gemini sem chave: não configurado.
    let (code, body) = context
        .call(
            "POST",
            "/v1/settings/embedding/test",
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "amostra", "settings": { "embedding.provider": "gemini", "embedding.model": "g" } })),
        )
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["error"]["details"]["missing"],
        json!(["embedding.api_key"])
    );
}

#[tokio::test]
async fn embedding_models_are_listed_where_the_provider_allows() {
    let context = TestContext::new().await;
    let provider = ScriptedLlm::start().await;
    let list = |body: Value| {
        let context = &context;
        async move {
            context
                .call(
                    "POST",
                    "/v1/settings/embedding/models",
                    Some(ADMIN_TOKEN),
                    Some(body),
                )
                .await
        }
    };
    let (code, body) = list(json!({ "provider": "local" })).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let e5 = body["models"]
        .as_array()
        .expect("models")
        .iter()
        .find(|model| model["id"] == "multilingual-e5-small")
        .expect("e5");
    assert_eq!(e5["dimensions"], 384);
    provider.push(
        200,
        json!({ "models": [{ "name": "bge-m3" }, { "name": "nomic-embed-text" }] }),
    );
    let (code, body) = list(json!({ "provider": "ollama", "base_url": provider.url })).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        body["models"][0],
        json!({ "id": "bge-m3", "dimensions": null })
    );
    assert_eq!(provider.received().pop().expect("tags").path, "/api/tags");
    provider.push(200, json!({ "data": [{ "id": "text-embedding-3-small" }] }));
    let (code, body) =
        list(json!({ "provider": "openai_compatible", "base_url": provider.url, "api_key": "k" }))
            .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let request = provider.received().pop().expect("models");
    assert_eq!(request.path, "/models");
    assert_eq!(request.headers["authorization"], "Bearer k");
    for name in ["gemini", "off"] {
        let (code, body) = list(json!({ "provider": name })).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "PROVIDER_LISTING_UNSUPPORTED");
    }
    let (code, body) = list(json!({ "provider": "ollama" })).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "PROVIDER_NOT_CONFIGURED");
}

#[tokio::test]
async fn status_shows_the_embedding_block() {
    let context = TestContext::new().await;
    let embedding = status(&context).await;
    assert_eq!(
        embedding,
        json!({
            "enabled": true,
            "configured": true,
            "missing": [],
            "provider": "local",
            "model": "multilingual-e5-small",
            "model_id": "local:multilingual-e5-small",
            "state": "ready",
            "dimensions": 384,
            "total": 0,
            "done": 0,
            "reindex_started_at": null,
            "local": null,
        })
    );
    set(
        &context,
        json!({ "embedding.provider": "ollama", "embedding.model": "x" }),
    )
    .await;
    // Sem `base_url`, a troca é recusada e nada muda.
    assert_eq!(status(&context).await["provider"], "local");
}

/// Carregador do modelo local controlado pelo teste: fica "baixando" até `release` e então
/// entrega uma sessão falsa (ou a falha roteirizada). Simula o download sem rede nem relógio.
/// O resultado da carga, liberado pelo teste.
type Gate = (
    std::sync::Mutex<Option<Result<(), String>>>,
    std::sync::Condvar,
);

#[derive(Clone, Default)]
struct GatedLoader {
    gate: Arc<Gate>,
}

impl GatedLoader {
    fn release(&self, result: Result<(), String>) {
        let (lock, condvar) = &*self.gate;
        *lock.lock().expect("gate") = Some(result);
        condvar.notify_all();
    }

    fn loader(&self) -> engramind::providers::embeddings::local::Loader {
        let gate = self.gate.clone();
        Arc::new(move |model, _path| {
            let (lock, condvar) = &*gate;
            let mut released = lock.lock().expect("gate");
            while released.is_none() {
                released = condvar.wait(released).expect("gate");
            }
            let dims = model.dims;
            released
                .clone()
                .expect("released")
                .map(|()| Box::new(FakeSession(dims)) as Box<dyn Session>)
        })
    }
}

struct FakeSession(usize);

impl Session for FakeSession {
    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>, String> {
        Ok(inputs
            .iter()
            .map(|_| (0..self.0).map(|index| (index % 7) as f32 + 0.5).collect())
            .collect())
    }
}

async fn wait_local_state(context: &TestContext, expected: &str) -> Value {
    for _ in 0..400 {
        let embedding = status(context).await;
        if embedding["local"]["state"] == expected {
            return embedding;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "modelo local nao chegou a {expected}: {}",
        status(context).await
    );
}

#[tokio::test]
async fn the_first_local_use_reports_the_download_instead_of_timing_out() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    let gate = GatedLoader::default();
    context
        .core
        .set_local_embedder(Arc::new(LocalEmbedder::with_loader(
            context.dir.path().join("models"),
            gate.loader(),
            std::time::Duration::from_millis(50),
        )));
    let id = memory(
        &context,
        &token,
        "eng/global",
        "Use cargo nextest para os testes",
    )
    .await;
    let embedding = status(&context).await;
    assert_eq!(embedding["local"]["model"], "multilingual-e5-small");
    assert_eq!(embedding["local"]["state"], "idle", "{embedding}");

    // A primeira chamada dispara o download e responde na hora, com o estado.
    let test = || {
        context.call(
            "POST",
            "/v1/settings/embedding/test",
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "amostra" })),
        )
    };
    let (code, body) = test().await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "EMBEDDING_MODEL_LOADING");
    let details = &body["error"]["details"];
    assert_eq!(details["model"], "multilingual-e5-small");
    assert_eq!(details["state"], "downloading");
    assert!(details["started_at"].is_string(), "{body}");
    assert!(details["elapsed_ms"].is_u64(), "{body}");
    let embedding = status(&context).await;
    assert_eq!(embedding["local"]["state"], "downloading", "{embedding}");
    assert!(embedding["local"]["started_at"].is_string());

    // O job `embed` espera o modelo sem gastar tentativa.
    run(&context).await;
    assert_eq!(embedding_state(&context, &id), "pending");
    let (state, attempts): (String, i64) = context
        .db()
        .query_row(
            "SELECT state, attempts FROM jobs WHERE kind = 'embed'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("job");
    assert_eq!((state.as_str(), attempts), ("queued", 0));

    gate.release(Ok(()));
    let embedding = wait_local_state(&context, "ready").await;
    assert!(embedding["local"]["elapsed_ms"].is_u64(), "{embedding}");
    let (code, body) = test().await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["dimensions"], 384);
    context
        .core
        .run_embed_jobs(chrono::Utc::now() + chrono::Duration::minutes(1))
        .await;
    assert_eq!(embedding_state(&context, &id), "ready");
}

#[tokio::test]
async fn a_failed_local_load_is_reported_by_the_call_and_the_status() {
    let context = TestContext::new().await;
    let gate = GatedLoader::default();
    context
        .core
        .set_local_embedder(Arc::new(LocalEmbedder::with_loader(
            context.dir.path().join("models"),
            gate.loader(),
            std::time::Duration::from_millis(50),
        )));
    let test = || {
        context.call(
            "POST",
            "/v1/settings/embedding/test",
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "amostra" })),
        )
    };
    let (code, body) = test().await;
    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    gate.release(Err("sem rede".to_owned()));
    let embedding = wait_local_state(&context, "failed").await;
    assert_eq!(embedding["local"]["error"], "sem rede", "{embedding}");
    let (code, body) = test().await;
    assert_eq!(code, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"]["code"], "PROVIDER_FAILED");
    assert_eq!(body["error"]["details"]["outcome"], "load_failed");
}

#[tokio::test]
async fn a_remote_rejection_returns_the_provider_reason() {
    let context = TestContext::new().await;
    let provider = ScriptedLlm::start().await;
    provider.push(
        400,
        json!({ "error": { "message": "model `x` is not an embedding model", "type": "invalid_request_error" } }),
    );
    let (code, body) = context
        .call(
            "POST",
            "/v1/settings/embedding/test",
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "amostra", "settings": {
                "embedding.provider": "openai_compatible",
                "embedding.base_url": provider.url,
                "embedding.model": "x",
            } })),
        )
        .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "PROVIDER_REJECTED");
    assert_eq!(
        body["error"]["details"]["reason"],
        "recusado pelo provedor (HTTP 400: model `x` is not an embedding model (invalid_request_error))"
    );
}
