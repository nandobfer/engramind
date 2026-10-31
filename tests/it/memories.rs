//! Memórias (etapa 004): invariantes de escrita na mesma transação, duplicata, conflito,
//! `revert`, `move`, `delete`, busca sem acentos, modos de assinatura, contadores,
//! configurações por canal e as três vias.

use std::process::{Command, Output, Stdio};

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::support::{ADMIN_TOKEN, TestContext};

/// App `app-a`, agente `curador` e um token dele com `read,write,delete`, assinando `eng/global`
/// (inject) e `eng/outro` (inject).
async fn setup(context: &TestContext) -> String {
    context.create_app("app-a").await;
    context.create_agent("app-a", "curador").await;
    context.create_channel("eng/global").await;
    context.create_channel("eng/outro").await;
    context
        .subscribe("app-a/curador", "eng/global", "inject", None)
        .await;
    context
        .subscribe("app-a/curador", "eng/outro", "inject", None)
        .await;
    context
        .issue(
            "app-a",
            Some("app-a/curador"),
            "agent",
            &["read", "write", "delete"],
        )
        .await
}

async fn channel_version(context: &TestContext, slug: &str) -> i64 {
    let (_, channel) = context
        .call(
            "GET",
            &format!("/v1/channels/{}", slug.replace('/', "%2F")),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    channel["version"].as_i64().expect("version")
}

async fn get(context: &TestContext, token: &str, id: &str) -> (StatusCode, Value) {
    context
        .call("GET", &format!("/v1/memories/{id}"), Some(token), None)
        .await
}

async fn post(
    context: &TestContext,
    token: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    context.call("POST", uri, Some(token), body).await
}

async fn search(context: &TestContext, token: &str, body: Value) -> Value {
    let (status, results) = post(context, token, "/v1/memories/search", Some(body)).await;
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

fn jobs(context: &TestContext, id: &str) -> Vec<(String, Value)> {
    let connection = context.db();
    let mut statement = connection
        .prepare("SELECT kind, payload_json FROM jobs ORDER BY id")
        .expect("prepare");
    statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("jobs")
        .map(|row| {
            let (kind, payload) = row.expect("row");
            (
                kind,
                serde_json::from_str::<Value>(&payload).expect("payload"),
            )
        })
        .filter(|(_, payload)| payload["memory_id"] == id)
        .collect()
}

#[tokio::test]
async fn create_writes_memory_revision_audit_versions_and_jobs_together() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    let before = channel_version(&context, "eng/global").await;
    let text = "Use nextest localmente. Rode a suíte completa só no CI.";
    let (status, created) = post(
        &context,
        &token,
        "/v1/memories",
        Some(json!({
            "channel": "eng/global",
            "text": format!("  {text}  "),
            "category": "CONSTRAINTS",
            "origin": { "source": "curator", "label": "caso 1" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["duplicate"], false);
    assert_eq!(created["similar"], json!([]));
    let memory = &created["memory"];
    let id = memory["id"].as_str().expect("id").to_owned();
    assert!(id.starts_with("m_"), "{id}");
    assert_eq!(memory["text"], text);
    assert_eq!(memory["title"], "Use nextest localmente");
    assert_eq!(memory["title_state"], "pending");
    assert_eq!(memory["embedding_state"], "pending");
    assert_eq!(memory["status"], "active");
    assert_eq!(memory["version"], 1);
    assert_eq!(memory["channel"], "eng/global");
    assert_eq!(memory["category"], "CONSTRAINTS");
    assert_eq!(memory["author"]["app"], "app-a");
    assert_eq!(memory["author"]["agent"], "app-a/curador");
    assert_eq!(
        memory["origin"],
        json!({ "source": "curator", "label": "caso 1" })
    );
    assert_eq!(memory["importance"], Value::Null);

    assert_eq!(channel_version(&context, "eng/global").await, before + 1);
    let jobs = jobs(&context, &id);
    assert_eq!(
        jobs.iter()
            .map(|(kind, _)| kind.as_str())
            .collect::<Vec<_>>(),
        vec!["redact", "embed"]
    );
    assert!(jobs.iter().all(|(_, payload)| payload["version"] == 1));

    let (_, history) = context
        .call(
            "GET",
            &format!("/v1/memories/{id}/history"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(history["revisions"].as_array().expect("revisions").len(), 1);
    assert_eq!(history["revisions"][0]["reason"], "create");
    assert_eq!(history["revisions"][0]["text"], text);
    assert_eq!(history["revisions"][0]["author"]["agent"], "app-a/curador");

    let (_, audit) = context
        .call("GET", "/v1/audit?entity=memory", Some(ADMIN_TOKEN), None)
        .await;
    let event = &audit["events"][0];
    assert_eq!(event["action"], "memory.create");
    assert_eq!(event["entity_id"], id.as_str());
    assert_eq!(
        event["after"]["content_hash"].as_str().expect("hash").len(),
        64
    );
    let serialized = audit.to_string();
    assert!(
        !serialized.contains("nextest"),
        "texto na auditoria: {serialized}"
    );

    // Uma falha no meio da transação desfaz tudo.
    context
        .db()
        .execute_batch(
            "CREATE TRIGGER falha_jobs BEFORE INSERT ON jobs BEGIN SELECT RAISE(ABORT, 'falha'); END;",
        )
        .expect("trigger");
    let memories = context.count("SELECT COUNT(*) FROM memories");
    let revisions = context.count("SELECT COUNT(*) FROM memory_revisions");
    let audits = context.count("SELECT COUNT(*) FROM audit_log");
    let version = channel_version(&context, "eng/global").await;
    let (status, failed) = post(
        &context,
        &token,
        "/v1/memories",
        Some(json!({ "channel": "eng/global", "text": "Outra memória qualquer" })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{failed}");
    assert_eq!(context.count("SELECT COUNT(*) FROM memories"), memories);
    assert_eq!(
        context.count("SELECT COUNT(*) FROM memory_revisions"),
        revisions
    );
    assert_eq!(context.count("SELECT COUNT(*) FROM audit_log"), audits);
    assert_eq!(channel_version(&context, "eng/global").await, version);
}

#[tokio::test]
async fn duplicates_and_idempotency_return_the_recorded_memory() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    let first = context
        .create_memory(&token, "eng/global", "Configuração do serviço fica no .env")
        .await;
    let id = first["memory"]["id"].clone();

    let (status, duplicate) = post(
        &context,
        &token,
        "/v1/memories",
        Some(json!({ "channel": "eng/global", "text": "configuração   do serviço FICA no .env" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{duplicate}");
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["memory"]["id"], id);
    assert_eq!(context.count("SELECT COUNT(*) FROM memories"), 1);
    assert_eq!(context.count("SELECT COUNT(*) FROM memory_revisions"), 1);
    assert_eq!(context.count("SELECT COUNT(*) FROM jobs"), 2);

    // Em outro canal, o mesmo texto é outra memória.
    let other = context
        .create_memory(&token, "eng/outro", "Configuração do serviço fica no .env")
        .await;
    assert_eq!(other["duplicate"], false);

    let body = json!({ "channel": "eng/global", "text": "Memória com chave de idempotência" });
    let headers = [("Idempotency-Key", "chave-1")];
    let (status, _, created) = context
        .call_with(
            "POST",
            "/v1/memories",
            Some(&token),
            Some(body.clone()),
            &headers,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, replay) = context
        .call_with("POST", "/v1/memories", Some(&token), Some(body), &headers)
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(replay, created);
    let (status, _, conflict) = context
        .call_with(
            "POST",
            "/v1/memories",
            Some(&token),
            Some(json!({ "channel": "eng/global", "text": "Outro corpo" })),
            &headers,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "IDEMPOTENCY_KEY_CONFLICT");

    // Duplicata com chave: 200 também na repetição.
    let body = json!({ "channel": "eng/global", "text": "Configuração do serviço fica no .env" });
    let headers = [("Idempotency-Key", "chave-2")];
    for _ in 0..2 {
        let (status, _, replay) = context
            .call_with(
                "POST",
                "/v1/memories",
                Some(&token),
                Some(body.clone()),
                &headers,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(replay["duplicate"], true);
    }

    // Sem `memory.dedupe_exact`, o mesmo texto vira outra memória.
    let (status, _) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "memory.dedupe_exact": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, again) = post(
        &context,
        &token,
        "/v1/memories",
        Some(json!({ "channel": "eng/global", "text": "Configuração do serviço fica no .env" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert_ne!(again["memory"]["id"], id);
}

#[tokio::test]
async fn writes_are_validated_before_touching_the_database() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    context.create_channel("eng/restrito").await;
    context.create_channel("eng/leitura").await;
    context
        .subscribe("app-a/curador", "eng/leitura", "inject", Some(&["read"]))
        .await;
    let create = |body: Value| {
        let context = &context;
        let token = token.clone();
        async move { post(context, &token, "/v1/memories", Some(body)).await }
    };

    let (status, error) = create(json!({ "channel": "eng/global", "text": "ab" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["details"]["field"], "text");
    assert_eq!(error["error"]["details"]["min"], 3);

    let (status, error) =
        create(json!({ "channel": "eng/global", "text": "abc", "category": "OUTRA" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "CATEGORY_NOT_FOUND");
    assert_eq!(error["error"]["details"]["valid"][0], "PROJECT_RULES");

    let (status, error) =
        create(json!({ "channel": "eng/global", "text": "abc", "status": "archived" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "VALIDATION_ERROR");

    let (status, error) = create(json!({ "channel": "eng/restrito", "text": "abc" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "CHANNEL_NOT_SUBSCRIBED");

    let (status, error) = create(json!({ "channel": "eng/leitura", "text": "abc" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "SCOPE_REQUIRED");
    assert_eq!(error["error"]["details"]["channel"], "eng/leitura");

    let (status, error) = create(json!({ "channel": "nao/existe", "text": "abc" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["error"]["code"], "CHANNEL_NOT_FOUND");

    let reader = context
        .issue("app-a", Some("app-a/curador"), "agent", &["read"])
        .await;
    let (status, error) = post(
        &context,
        &reader,
        "/v1/memories",
        Some(json!({ "channel": "eng/global", "text": "abc" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "SCOPE_REQUIRED");

    // `memory.text_max` sobreposto no canal.
    let (status, settings) = context
        .call(
            "PUT",
            "/v1/channels/eng%2Fglobal/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "memory.text_max": 10 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    let (status, error) = create(json!({ "channel": "eng/global", "text": "12345678901" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["details"]["max"], 10);
    let (status, _) = create(json!({ "channel": "eng/outro", "text": "12345678901" })).await;
    assert_eq!(status, StatusCode::CREATED);

    // Canal arquivado não recebe escrita.
    let (status, _) = post(
        &context,
        ADMIN_TOKEN,
        "/v1/channels/eng%2Foutro/archive",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, error) = create(json!({ "channel": "eng/outro", "text": "abcdef" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "CHANNEL_ARCHIVED");
    assert_eq!(context.count("SELECT COUNT(*) FROM memories"), 1);
}

#[tokio::test]
async fn updates_are_versioned_with_conflicts_and_duplicates() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    let first = context
        .create_memory(&token, "eng/global", "Use tabs na indentação")
        .await;
    let second = context
        .create_memory(&token, "eng/global", "Use espaços na indentação")
        .await;
    let id = first["memory"]["id"].as_str().expect("id").to_owned();
    let uri = format!("/v1/memories/{id}");
    let channel = channel_version(&context, "eng/global").await;

    let (status, updated) = context
        .call(
            "PATCH",
            &uri,
            Some(&token),
            Some(json!({ "text": "Use quatro espaços na indentação. Nunca tabs.", "expected_version": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["memory"]["version"], 2);
    assert_eq!(
        updated["memory"]["title"],
        "Use quatro espaços na indentação"
    );
    assert_eq!(updated["memory"]["title_state"], "pending");
    assert_eq!(channel_version(&context, "eng/global").await, channel + 1);
    let queued = jobs(&context, &id);
    assert_eq!(queued.len(), 4);
    assert_eq!(queued[3].1["version"], 2);

    let (status, conflict) = context
        .call(
            "PATCH",
            &uri,
            Some(&token),
            Some(json!({ "text": "Outra coisa", "expected_version": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "MEMORY_VERSION_CONFLICT");
    assert_eq!(conflict["error"]["details"]["current_version"], 2);

    let (status, duplicate) = context
        .call(
            "PATCH",
            &uri,
            Some(&token),
            Some(json!({ "text": "use espaços na indentação" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(duplicate["error"]["code"], "MEMORY_DUPLICATE");
    assert_eq!(
        duplicate["error"]["details"]["existing_id"],
        second["memory"]["id"]
    );

    // Só a categoria: revisão nova, sem reprocessar.
    let (status, recategorized) = context
        .call(
            "PATCH",
            &uri,
            Some(&token),
            Some(json!({ "category": "NAMING" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recategorized["memory"]["version"], 3);
    assert_eq!(recategorized["memory"]["category"], "NAMING");
    assert_eq!(jobs(&context, &id).len(), 4);

    // Sem mudança efetiva, nada é gravado.
    let (status, same) = context
        .call(
            "PATCH",
            &uri,
            Some(&token),
            Some(json!({ "category": "NAMING" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(same["memory"]["version"], 3);
    assert_eq!(
        context.count("SELECT COUNT(*) FROM memory_revisions WHERE reason = 'update'"),
        2
    );

    let (status, error) = context
        .call("PATCH", &uri, Some(&token), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    let (_, history) = context
        .call("GET", &format!("{uri}/history"), Some(&token), None)
        .await;
    let versions: Vec<i64> = history["revisions"]
        .as_array()
        .expect("revisions")
        .iter()
        .map(|revision| revision["version"].as_i64().expect("version"))
        .collect();
    assert_eq!(versions, vec![3, 2, 1]);
}

#[tokio::test]
async fn status_move_revert_and_delete_follow_the_invariants() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    let created = context
        .create_memory(&token, "eng/global", "Deploy só na main")
        .await;
    let id = created["memory"]["id"].as_str().expect("id").to_owned();
    let uri = format!("/v1/memories/{id}");

    let (status, archived) = post(&context, &token, &format!("{uri}/archive"), None).await;
    assert_eq!(status, StatusCode::OK, "{archived}");
    assert_eq!(archived["status"], "archived");
    assert!(archived["archived_at"].is_string());
    assert_eq!(archived["version"], 2);
    let (_, again) = post(&context, &token, &format!("{uri}/archive"), None).await;
    assert_eq!(again["version"], 2);
    let (_, restored) = post(&context, &token, &format!("{uri}/restore"), None).await;
    assert_eq!(restored["status"], "active");
    assert_eq!(restored["archived_at"], Value::Null);
    let (_, pinned) = post(&context, &token, &format!("{uri}/pin"), None).await;
    assert_eq!(pinned["status"], "permanent");
    let (_, unpinned) = post(&context, &token, &format!("{uri}/unpin"), None).await;
    assert_eq!(unpinned["status"], "active");
    assert_eq!(unpinned["version"], 5);

    // Revert: conteúdo antigo numa revisão nova; o Redator só refaz o título.
    context
        .call(
            "PATCH",
            &uri,
            Some(&token),
            Some(json!({ "text": "Deploy pela tag" })),
        )
        .await;
    let (status, reverted) = post(
        &context,
        &token,
        &format!("{uri}/revert"),
        Some(json!({ "version": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reverted}");
    assert_eq!(reverted["text"], "Deploy só na main");
    assert_eq!(reverted["version"], 7);
    let last = jobs(&context, &id);
    let redact = &last[last.len() - 2];
    assert_eq!(redact.0, "redact");
    assert_eq!(redact.1["title_only"], true);
    assert_eq!(redact.1["version"], 7);
    let (status, missing) = post(
        &context,
        &token,
        &format!("{uri}/revert"),
        Some(json!({ "version": 99 })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["code"], "REVISION_NOT_FOUND");
    assert_eq!(
        context.count("SELECT COUNT(*) FROM memory_revisions"),
        7,
        "uma revisão por mudança"
    );

    // Move: write nos dois canais e as duas versões de canal.
    let (global, outro) = (
        channel_version(&context, "eng/global").await,
        channel_version(&context, "eng/outro").await,
    );
    let (status, moved) = post(
        &context,
        &token,
        &format!("{uri}/move"),
        Some(json!({ "to": "eng/outro" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["channel"], "eng/outro");
    assert_eq!(channel_version(&context, "eng/global").await, global + 1);
    assert_eq!(channel_version(&context, "eng/outro").await, outro + 1);
    context.create_channel("eng/fechado").await;
    let (status, error) = post(
        &context,
        &token,
        &format!("{uri}/move"),
        Some(json!({ "to": "eng/fechado" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "CHANNEL_NOT_SUBSCRIBED");
    let twin = context
        .create_memory(&token, "eng/global", "Deploy só na main")
        .await;
    let (status, error) = post(
        &context,
        &token,
        &format!(
            "/v1/memories/{}/move",
            twin["memory"]["id"].as_str().expect("id")
        ),
        Some(json!({ "to": "eng/outro" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "MEMORY_DUPLICATE");
    assert_eq!(error["error"]["details"]["existing_id"], id.as_str());

    // Canal arquivado: só leitura e exclusão.
    post(
        &context,
        ADMIN_TOKEN,
        "/v1/channels/eng%2Foutro/archive",
        None,
    )
    .await;
    let (status, error) = post(&context, &token, &format!("{uri}/pin"), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "CHANNEL_ARCHIVED");
    assert_eq!(get(&context, &token, &id).await.0, StatusCode::OK);

    let (status, deleted) = context.call("DELETE", &uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{deleted}");
    assert_eq!(get(&context, &token, &id).await.0, StatusCode::NOT_FOUND);
    let last_revision: i64 = context
        .db()
        .query_row(
            "SELECT version FROM memory_revisions WHERE memory_id = ?1",
            [&id],
            |row| row.get(0),
        )
        .expect("ultima revisao");
    assert_eq!(last_revision, 8);
    assert_eq!(
        context.count("SELECT COUNT(*) FROM memory_fts WHERE memory_fts MATCH 'deploy'"),
        1,
        "só a gêmea continua no índice"
    );
    let (_, audit) = context
        .call(
            "GET",
            "/v1/audit?action=memory.delete",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(audit["events"][0]["entity_id"], id.as_str());
}

#[tokio::test]
async fn search_ignores_accents_and_respects_modes_and_filters() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_agent("app-a", "agente").await;
    for (slug, mode) in [
        ("eng/global", "inject"),
        ("eng/busca", "search"),
        ("eng/gestao", "manage"),
    ] {
        context.create_channel(slug).await;
        context.subscribe("app-a/agente", slug, mode, None).await;
    }
    let token = context
        .issue("app-a", Some("app-a/agente"), "agent", &["read", "write"])
        .await;
    let global = context
        .create_memory(&token, "eng/global", "A configuração do banco usa WAL")
        .await["memory"]["id"]
        .clone();
    let busca = context
        .create_memory(&token, "eng/busca", "Configuração de cache em memória")
        .await["memory"]["id"]
        .clone();
    let gestao = context
        .create_memory(&token, "eng/gestao", "Configuração restrita da gestão")
        .await["memory"]["id"]
        .clone();
    let (status, _) = context
        .call(
            "PATCH",
            &format!("/v1/memories/{}", busca.as_str().expect("id")),
            Some(&token),
            Some(json!({ "category": "NAMING" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let found = ids(&search(&context, &token, json!({ "query": "configuracao" })).await);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.contains(&global.as_str().expect("id").to_owned()));
    assert!(found.contains(&busca.as_str().expect("id").to_owned()));

    let cited = search(
        &context,
        &token,
        json!({ "query": "configuração", "channels": ["eng/gestao"] }),
    )
    .await;
    assert_eq!(ids(&cited), vec![gestao.as_str().expect("id").to_owned()]);
    assert_eq!(cited["results"][0]["rank"], 1);
    assert!(cited["results"][0]["score"].as_f64().expect("score") > 0.0);

    let by_category = search(
        &context,
        &token,
        json!({ "query": "configuracao", "categories": ["NAMING"], "fields": "title" }),
    )
    .await;
    assert_eq!(
        ids(&by_category),
        vec![busca.as_str().expect("id").to_owned()]
    );
    let item = by_category["results"][0]["memory"]
        .as_object()
        .expect("item");
    assert!(
        item.contains_key("title") && !item.contains_key("text"),
        "{item:?}"
    );

    let excluded = search(
        &context,
        &token,
        json!({ "query": "configuracao", "exclude_ids": [global] }),
    )
    .await;
    assert_eq!(ids(&excluded), vec![busca.as_str().expect("id").to_owned()]);

    // Arquivadas só com `include_archived`.
    post(
        &context,
        &token,
        &format!("/v1/memories/{}/archive", global.as_str().expect("id")),
        None,
    )
    .await;
    assert_eq!(
        ids(&search(&context, &token, json!({ "query": "wal" })).await).len(),
        0
    );
    assert_eq!(
        ids(&search(
            &context,
            &token,
            json!({ "query": "wal", "include_archived": true })
        )
        .await)
        .len(),
        1
    );

    // O índice acompanha a edição.
    context
        .call(
            "PATCH",
            &format!("/v1/memories/{}", busca.as_str().expect("id")),
            Some(&token),
            Some(json!({ "text": "Redis guarda as sessões" })),
        )
        .await;
    assert!(ids(&search(&context, &token, json!({ "query": "cache" })).await).is_empty());
    assert_eq!(
        ids(&search(&context, &token, json!({ "query": "sessoes" })).await),
        vec![busca.as_str().expect("id").to_owned()]
    );

    // Contadores em lote, sem mudar a versão.
    let (_, before) = get(&context, &token, busca.as_str().expect("id")).await;
    context.core.flush_counters().await.expect("flush");
    let (_, after) = get(&context, &token, busca.as_str().expect("id")).await;
    assert!(
        after["retrieval_count"].as_i64().expect("count") >= 4,
        "{after}"
    );
    assert!(after["last_retrieved_at"].is_string());
    assert_eq!(after["version"], before["version"]);
    assert_eq!(after["updated_at"], before["updated_at"]);

    let (status, error) = post(
        &context,
        &token,
        "/v1/memories/search",
        Some(json!({ "query": "  ...  " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let (status, _) = post(
        &context,
        &token,
        "/v1/memories/search",
        Some(json!({ "query": "x", "limit": 51 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn lists_filter_and_unreachable_memories_look_missing() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    context.create_agent("app-a", "outro").await;
    context.create_channel("eng/secreto").await;
    let other = context
        .issue("app-a", Some("app-a/outro"), "agent", &["read", "write"])
        .await;
    context
        .subscribe("app-a/outro", "eng/secreto", "inject", None)
        .await;
    let secret = context
        .create_memory(&other, "eng/secreto", "Memória que o curador não alcança")
        .await["memory"]["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let (status, error) = get(&context, &token, &secret).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["error"]["code"], "MEMORY_NOT_FOUND");
    let (status, _) = context
        .call(
            "DELETE",
            &format!("/v1/memories/{secret}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let a = context
        .create_memory(&token, "eng/global", "Primeira do global")
        .await;
    let b = context
        .create_memory(&token, "eng/outro", "Segunda do outro")
        .await;
    let c = context
        .create_memory(&token, "eng/global", "Terceira do global")
        .await;
    let id = |value: &Value| value["memory"]["id"].as_str().expect("id").to_owned();
    post(
        &context,
        &token,
        &format!("/v1/memories/{}/archive", id(&c)),
        None,
    )
    .await;

    let list = |query: &'static str| {
        let context = &context;
        let token = token.clone();
        async move {
            let (status, list) = context
                .call("GET", &format!("/v1/memories{query}"), Some(&token), None)
                .await;
            assert_eq!(status, StatusCode::OK, "{list}");
            list
        }
    };
    let listed = |list: &Value| -> Vec<String> {
        list["memories"]
            .as_array()
            .expect("memories")
            .iter()
            .map(|memory| memory["id"].as_str().expect("id").to_owned())
            .collect()
    };

    assert_eq!(listed(&list("").await), vec![id(&b), id(&a)]);
    assert_eq!(listed(&list("?status=archived").await), vec![id(&c)]);
    assert_eq!(
        listed(&list("?channels=eng/global&status=active,archived").await),
        vec![id(&c), id(&a)]
    );
    assert_eq!(
        listed(&list("?author=app-a/curador").await),
        vec![id(&b), id(&a)]
    );
    assert_eq!(listed(&list("?author=app-a").await), vec![id(&b), id(&a)]);
    assert!(listed(&list("?author=app-a/ninguem").await).is_empty());
    assert!(listed(&list("?since=2999-01-01T00:00:00Z").await).is_empty());
    let titles = list("?fields=title").await;
    let item = titles["memories"][0].as_object().expect("item");
    assert_eq!(
        item.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["category", "channel", "id", "status", "title"]
    );
    let first = list("?limit=1").await;
    assert_eq!(listed(&first), vec![id(&b)]);
    let cursor = first["next_cursor"].as_str().expect("cursor");
    let (_, second) = context
        .call(
            "GET",
            &format!("/v1/memories?limit=1&cursor={cursor}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(listed(&second), vec![id(&a)]);
    assert_eq!(second["next_cursor"], Value::Null);

    // O cabeçalho da conexão restringe também a leitura por id.
    let (status, _, error) = context
        .call_with(
            "GET",
            &format!("/v1/memories/{}", id(&b)),
            Some(&token),
            None,
            &[("Engramind-Channels", "eng/global")],
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{error}");
}

#[tokio::test]
async fn similar_suggests_near_duplicates_with_a_configurable_threshold() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    let first = context
        .create_memory(
            &token,
            "eng/global",
            "alfa beta gama delta epsilon zeta eta teta iota kapa",
        )
        .await;
    let second = context
        .create_memory(
            &token,
            "eng/global",
            "alfa beta gama delta epsilon zeta eta teta iota lambda",
        )
        .await;
    assert_eq!(second["similar"], json!([]), "Jaccard 9/11 abaixo de 0,85");
    let (status, _) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "memory.similar_threshold": 0.8 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let third = context
        .create_memory(
            &token,
            "eng/global",
            "alfa beta gama delta epsilon zeta eta teta iota mi",
        )
        .await;
    let similar = third["similar"].as_array().expect("similar");
    assert_eq!(similar.len(), 2, "{similar:?}");
    assert_eq!(similar[0]["score"], 0.818);
    assert!(
        similar
            .iter()
            .any(|item| item["id"] == first["memory"]["id"])
    );
    // Na edição, a própria memória fica de fora.
    let (_, updated) = context
        .call(
            "PATCH",
            &format!(
                "/v1/memories/{}",
                third["memory"]["id"].as_str().expect("id")
            ),
            Some(&token),
            Some(json!({ "category": "NAMING" })),
        )
        .await;
    assert!(
        updated["similar"]
            .as_array()
            .expect("similar")
            .iter()
            .all(|item| item["id"] != third["memory"]["id"])
    );
}

#[tokio::test]
async fn channel_settings_stats_categories_and_channel_deletion() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    let (status, view) = context
        .call(
            "GET",
            "/v1/channels/eng%2Fglobal/settings",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["settings"][0]["key"], "memory.text_max");
    assert_eq!(view["settings"][0]["value"], 4000);
    assert_eq!(view["settings"][0]["override"], Value::Null);

    let version = channel_version(&context, "eng/global").await;
    let (status, view) = context
        .call(
            "PUT",
            "/v1/channels/eng%2Fglobal/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "memory.text_max": 200 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["settings"][0]["value"], 200);
    assert_eq!(view["settings"][0]["global"], 4000);
    assert_eq!(view["settings"][0]["override"], 200);
    assert_eq!(channel_version(&context, "eng/global").await, version + 1);
    let (status, error) = context
        .call(
            "PUT",
            "/v1/channels/eng%2Fglobal/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "memory.text_min": 5 })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "SETTING_UNKNOWN");
    assert_eq!(
        error["error"]["details"]["valid"],
        json!([
            "memory.text_max",
            "context.budget_tokens",
            "context.titles_budget_tokens"
        ])
    );
    let (_, view) = context
        .call(
            "PUT",
            "/v1/channels/eng%2Fglobal/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "memory.text_max": null })),
        )
        .await;
    assert_eq!(view["settings"][0]["override"], Value::Null);
    let (status, _) = context
        .call(
            "GET",
            "/v1/channels/eng%2Fglobal/settings",
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let a = context
        .create_memory(&token, "eng/global", "Memória um")
        .await;
    context
        .create_memory(&token, "eng/global", "Memória dois")
        .await;
    post(
        &context,
        &token,
        &format!(
            "/v1/memories/{}/pin",
            a["memory"]["id"].as_str().expect("id")
        ),
        None,
    )
    .await;
    context
        .call(
            "PATCH",
            &format!("/v1/memories/{}", a["memory"]["id"].as_str().expect("id")),
            Some(&token),
            Some(json!({ "category": "NAMING" })),
        )
        .await;
    let (status, stats) = context
        .call("GET", "/v1/channels/eng%2Fglobal/stats", Some(&token), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{stats}");
    assert_eq!(
        stats["memories"],
        json!({ "total": 2, "active": 1, "permanent": 1, "archived": 0 })
    );
    assert_eq!(
        stats["by_category"],
        json!({ "NAMING": 1, "PROJECT_RULES": 1 })
    );
    assert_eq!(stats["pending"], json!({ "title": 2, "embedding": 2 }));
    assert_eq!(
        stats["version"],
        channel_version(&context, "eng/global").await
    );

    // Categoria em uso não sai da lista; label e prioridade mudam livremente.
    let (status, error) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "categories.list": [{ "key": "PROJECT_RULES", "label": "Regras", "priority": 1 }] })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["error"]["code"], "CATEGORY_IN_USE");
    assert_eq!(error["error"]["details"]["in_use"], json!({ "NAMING": 1 }));
    let (status, _) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "categories.list": [
                { "key": "NAMING", "label": "Nomes", "priority": 99 },
                { "key": "PROJECT_RULES", "label": "Regras", "priority": 1 }
            ] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, created) = post(
        &context,
        &token,
        "/v1/memories",
        Some(json!({ "channel": "eng/global", "text": "Categoria padrão nova" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["memory"]["category"], "NAMING");

    // Excluir o canal leva as memórias e as revisões.
    post(
        &context,
        ADMIN_TOKEN,
        "/v1/channels/eng%2Fglobal/archive",
        None,
    )
    .await;
    let (status, _) = context
        .call(
            "DELETE",
            "/v1/channels/eng%2Fglobal",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(context.count("SELECT COUNT(*) FROM memories"), 0);
    assert_eq!(context.count("SELECT COUNT(*) FROM memory_revisions"), 0);
    let (_, audit) = context
        .call(
            "GET",
            "/v1/audit?action=channel.delete",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(audit["events"][0]["before"]["memories_removed"], 3);
    let report = engramind::store::Store::diagnose(&context.store)
        .await
        .expect("diagnose");
    assert_eq!(report.memory_fts.as_deref(), Some("ok"));
}

// ---------- três vias ----------

fn bin() -> String {
    std::env::var("NEXTEST_BIN_EXE_engramind")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_engramind").to_owned())
}

async fn cli(
    context: &TestContext,
    url: &str,
    token: &str,
    args: &[&str],
    stdin: Option<&str>,
) -> Output {
    let home = context.dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let mut command = Command::new(bin());
    command
        .args(args)
        .arg("--json")
        .env_clear()
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("ENGRAMIND_URL", url)
        .env("ENGRAMIND_TOKEN", token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let input = stdin.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let mut child = command.spawn().expect("spawn engramind");
        if let Some(input) = input {
            use std::io::Write as _;
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(input.as_bytes())
                .expect("write stdin");
        } else {
            drop(child.stdin.take());
        }
        child.wait_with_output().expect("run engramind")
    })
    .await
    .expect("join")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_and_cli_share_the_memory_contract() {
    let context = TestContext::new().await;
    let token = setup(&context).await;
    let url = context.serve().await;

    // CLI: texto multilinha pela entrada padrão.
    let output = cli(
        &context,
        &url,
        &token,
        &[
            "memories",
            "create",
            "--channel",
            "eng/global",
            "--text",
            "-",
            "--category",
            "ARCHITECTURE",
        ],
        Some("Filas ficam na tabela jobs.\nSem broker externo.\n"),
    )
    .await;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    let created: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(
        created["memory"]["text"],
        "Filas ficam na tabela jobs.\nSem broker externo."
    );
    assert_eq!(created["memory"]["title"], "Filas ficam na tabela jobs");
    let id = created["memory"]["id"].as_str().expect("id").to_owned();

    // MCP: busca, edição com conflito e o mesmo envelope de erro.
    let found = context
        .call_tool(&token, "search_memories", json!({ "query": "fila broker" }))
        .await;
    assert_eq!(found["result"]["isError"], false, "{found}");
    assert_eq!(
        found["result"]["structuredContent"]["results"][0]["memory"]["id"],
        id.as_str()
    );
    let updated = context
        .call_tool(
            &token,
            "update_memory",
            json!({ "memory": id, "text": "Filas ficam na tabela jobs, sem broker.", "expected_version": 1 }),
        )
        .await;
    assert_eq!(
        updated["result"]["structuredContent"]["memory"]["version"], 2,
        "{updated}"
    );
    let conflict = context
        .call_tool(
            &token,
            "update_memory",
            json!({ "memory": id, "text": "Outra", "expected_version": 1 }),
        )
        .await;
    assert_eq!(conflict["result"]["isError"], true);
    assert_eq!(
        conflict["result"]["structuredContent"]["error"]["code"],
        "MEMORY_VERSION_CONFLICT"
    );
    let duplicate = context
        .call_tool(
            &token,
            "create_memory",
            json!({ "channel": "eng/global", "text": "Filas ficam na tabela jobs, sem broker.", "idempotency_key": "k-1" }),
        )
        .await;
    assert_eq!(
        duplicate["result"]["structuredContent"]["duplicate"], true,
        "{duplicate}"
    );

    // CLI: revert e o código de saída de um conflito.
    let output = cli(
        &context,
        &url,
        &token,
        &["memories", "revert", &id, "--version", "1"],
        None,
    )
    .await;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    let reverted: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(reverted["version"], 3);
    let output = cli(
        &context,
        &url,
        &token,
        &[
            "memories",
            "update",
            &id,
            "--text",
            "Mais uma",
            "--expected-version",
            "1",
        ],
        None,
    )
    .await;
    assert_eq!(output.status.code(), Some(6));
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("envelope");
    assert_eq!(envelope["error"]["code"], "MEMORY_VERSION_CONFLICT");

    let output = cli(
        &context,
        &url,
        &token,
        &["memories", "search", "tabela", "jobs", "--fields", "title"],
        None,
    )
    .await;
    let results: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(results["results"][0]["memory"]["id"], id.as_str());
    assert!(results["results"][0]["memory"].get("text").is_none());

    let output = cli(
        &context,
        &url,
        &token,
        &["channels", "stats", "eng/global"],
        None,
    )
    .await;
    let stats: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(stats["memories"]["total"], 1);
}
