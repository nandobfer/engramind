//! Integração (008): `import_memories`, `get_usage`, `get_status` completo, `/metrics` e a
//! retenção.

use std::process::{Command, Output};

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use engramind::core::ids;
use serde_json::{Value, json};

use crate::support::{ADMIN_TOKEN, TestContext};

const CHANNEL: &str = "eng/global";

fn bin() -> String {
    std::env::var("NEXTEST_BIN_EXE_engramind")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_engramind").to_owned())
}

async fn cli(context: &TestContext, url: &str, args: &[&str]) -> Output {
    let home = context.dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let mut command = Command::new(bin());
    command
        .args(args)
        .arg("--json")
        .current_dir(context.dir.path())
        .env_clear()
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("ENGRAMIND_URL", url)
        .env("ENGRAMIND_TOKEN", ADMIN_TOKEN);
    tokio::task::spawn_blocking(move || command.output().expect("run engramind"))
        .await
        .expect("join")
}

async fn import(context: &TestContext, body: Value, key: Option<&str>) -> (StatusCode, Value) {
    let headers: Vec<(&str, &str)> = key
        .map(|key| ("Idempotency-Key", key))
        .into_iter()
        .collect();
    let (status, _, body) = context
        .call_with(
            "POST",
            "/v1/memories/import",
            Some(ADMIN_TOKEN),
            Some(body),
            &headers,
        )
        .await;
    (status, body)
}

async fn setup() -> TestContext {
    let context = TestContext::new().await;
    context.create_channel(CHANNEL).await;
    context.create_app("legado").await;
    context.create_agent("legado", "curador").await;
    context
}

#[tokio::test]
async fn import_preserves_dates_and_authors_and_reports_duplicates() {
    let context = setup().await;
    let existing = context
        .create_memory(ADMIN_TOKEN, CHANNEL, "Memoria que ja existia no canal")
        .await;
    let mut memories: Vec<Value> = (0..98)
        .map(|n| {
            json!({
                "text": format!("Fato importado numero {n} do sistema legado"),
                "category": "CONSTRAINTS",
                "created_at": format!("2026-01-{:02}T12:00:00Z", n % 28 + 1),
                "author": { "app": "legado", "agent": "curador" },
                "origin": { "source": "import", "label": format!("legado#{n}") },
            })
        })
        .collect();
    memories.push(json!({ "text": "Memoria que ja existia no canal" }));
    memories.push(json!({ "text": "Fato importado numero 0 do sistema legado" }));
    let body = json!({ "channel": CHANNEL, "memories": memories });
    let jobs_before = context.count("SELECT count(*) FROM jobs");

    let (status, result) = import(&context, body.clone(), Some("import-1")).await;
    assert_eq!(status, StatusCode::CREATED, "{result}");
    assert_eq!(result["created"].as_array().expect("created").len(), 98);
    assert_eq!(
        result["duplicates"],
        json!([
            { "index": 98, "existing_id": existing["memory"]["id"] },
            { "index": 99, "existing_id": result["created"][0]["id"] },
        ])
    );
    let first = result["created"][0]["id"].as_str().expect("id");
    let (status, memory) = context
        .call(
            "GET",
            &format!("/v1/memories/{first}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(memory["created_at"], "2026-01-01T12:00:00.000Z");
    assert_eq!(memory["updated_at"], "2026-01-01T12:00:00.000Z");
    assert_eq!(memory["author"]["agent"], "legado/curador", "{memory}");
    assert_eq!(memory["author"]["app"], "legado");
    assert_eq!(
        memory["origin"],
        json!({ "source": "import", "label": "legado#0" })
    );
    assert_eq!(memory["version"], 1);
    let (_, history) = context
        .call(
            "GET",
            &format!("/v1/memories/{first}/history"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    let revision = &history["revisions"][0];
    assert_eq!(revision["reason"], "import", "{history}");
    assert_eq!(revision["created_at"], "2026-01-01T12:00:00.000Z");
    // `redact` e `embed` por memória criada; uma auditoria do lote, sem textos.
    assert_eq!(
        context.count("SELECT count(*) FROM jobs") - jobs_before,
        98 * 2
    );
    let audit: String = context
        .db()
        .query_row(
            "SELECT after_json FROM audit_log WHERE action = 'memory.import'",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    assert!(!audit.contains("Fato importado"), "{audit}");

    // Repetir a chave devolve a mesma resposta, sem gravar nem enfileirar.
    let jobs = context.count("SELECT count(*) FROM jobs");
    let (status, again) = import(&context, body.clone(), Some("import-1")).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(again, result);
    assert_eq!(context.count("SELECT count(*) FROM jobs"), jobs);
    assert_eq!(context.count("SELECT count(*) FROM memories"), 99);

    // A mesma chave com outro corpo é conflito.
    let (status, conflict) = import(
        &context,
        json!({ "channel": CHANNEL, "memories": [{ "text": "outro corpo qualquer" }] }),
        Some("import-1"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
}

#[tokio::test]
async fn import_validates_everything_first_and_requires_a_key() {
    let context = setup().await;
    let one = json!({ "channel": CHANNEL, "memories": [{ "text": "Um fato valido qualquer" }] });

    let (status, body) = import(&context, one.clone(), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "IDEMPOTENCY_KEY_REQUIRED");
    let tool = context
        .call_tool(
            ADMIN_TOKEN,
            "import_memories",
            json!({ "channel": CHANNEL, "memories": [{ "text": "Um fato valido qualquer" }] }),
        )
        .await;
    assert_eq!(
        tool["result"]["structuredContent"]["error"]["code"],
        "IDEMPOTENCY_KEY_REQUIRED"
    );

    let cases = [
        (
            json!([{ "text": "valido" }, { "text": "x" }]),
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
        ),
        (
            json!([{ "text": "valido" }, { "text": "no futuro", "created_at": "2999-01-01T00:00:00Z" }]),
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
        ),
        (
            json!([{ "text": "valido" }, { "text": "categoria", "category": "NADA" }]),
            StatusCode::BAD_REQUEST,
            "CATEGORY_NOT_FOUND",
        ),
        (
            json!([{ "text": "valido" }, { "text": "autor", "author": { "app": "nao-existe" } }]),
            StatusCode::NOT_FOUND,
            "APP_NOT_FOUND",
        ),
        (
            json!([{ "text": "valido" }, { "text": "autor", "author": { "app": "legado", "agent": "outro" } }]),
            StatusCode::NOT_FOUND,
            "AGENT_NOT_FOUND",
        ),
    ];
    for (n, (memories, expected_status, code)) in cases.into_iter().enumerate() {
        let (status, body) = import(
            &context,
            json!({ "channel": CHANNEL, "memories": memories }),
            Some(&format!("invalido-{n}")),
        )
        .await;
        assert_eq!(status, expected_status, "{body}");
        assert_eq!(body["error"]["code"], code);
        assert_eq!(body["error"]["details"]["index"], 1, "{body}");
    }
    assert_eq!(
        context.count("SELECT count(*) FROM memories"),
        0,
        "tudo ou nada"
    );

    let (status, body) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "limits.max_batch": 2 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let three: Vec<Value> = (0..3)
        .map(|n| json!({ "text": format!("fato {n} do lote") }))
        .collect();
    let (status, body) = import(
        &context,
        json!({ "channel": CHANNEL, "memories": three }),
        Some("grande"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "BATCH_TOO_LARGE");
    assert_eq!(body["error"]["details"], json!({ "max": 2, "received": 3 }));

    // Agente não importa.
    let token = context
        .issue(
            "legado",
            Some("legado/curador"),
            "agent",
            &["read", "write"],
        )
        .await;
    let (status, _, body) = context
        .call_with(
            "POST",
            "/v1/memories/import",
            Some(&token),
            Some(one),
            &[("Idempotency-Key", "agente")],
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn the_cli_imports_jsonl_with_a_derived_key() {
    let context = setup().await;
    let url = context.serve().await;
    let file = context.dir.path().join("memorias.jsonl");
    std::fs::write(
        &file,
        "{\"text\": \"Primeiro fato vindo do arquivo\"}\n{\"text\": \"Segundo fato vindo do arquivo\", \"status\": \"permanent\"}\n",
    )
    .expect("arquivo");
    let path = file.display().to_string();
    let args = [
        "memories",
        "import",
        "--channel",
        CHANNEL,
        "--file",
        path.as_str(),
    ];
    let first = cli(&context, &url, &args).await;
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(first.status.success(), "{stdout}");
    let first: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(first["created"].as_array().expect("created").len(), 2);
    let again = cli(&context, &url, &args).await;
    let again: Value = serde_json::from_slice(&again.stdout).expect("json");
    assert_eq!(again, first, "a chave derivada repete a resposta");
    assert_eq!(context.count("SELECT count(*) FROM memories"), 2);
}

#[tokio::test]
async fn usage_aggregates_by_day_agent_operation_and_channel() {
    let context = setup().await;
    context.create_channel("ops/geral").await;
    context
        .subscribe("legado/curador", CHANNEL, "inject", None)
        .await;
    context
        .subscribe("legado/curador", "ops/geral", "inject", None)
        .await;
    let token = context
        .issue(
            "legado",
            Some("legado/curador"),
            "agent",
            &["read", "write"],
        )
        .await;
    context
        .create_memory(&token, CHANNEL, "Fato do canal global para o uso")
        .await;
    context
        .create_memory(&token, "ops/geral", "Fato do canal de operacoes para o uso")
        .await;
    for _ in 0..2 {
        let (status, body) = context.call("GET", "/v1/context", Some(&token), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (status, _) = context
        .call(
            "POST",
            "/v1/memories/search",
            Some(&token),
            Some(json!({ "query": "fato canal" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    context.core.flush_counters().await.expect("flush");

    let usage = |group: &str| {
        let context = &context;
        let uri = format!("/v1/usage?group_by={group}");
        async move {
            let (status, body) = context.call("GET", &uri, Some(ADMIN_TOKEN), None).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            body
        }
    };
    let day = usage("day").await;
    assert_eq!(day["group_by"], "day");
    assert_eq!(day["total"]["requests"], 3, "{day}");
    assert_eq!(day["rows"][0]["key"], ids::timestamp(Utc::now())[..10]);
    assert!(day["total"]["rendered_tokens"].as_u64().expect("tokens") > 0);

    let agent = usage("agent").await;
    assert_eq!(agent["rows"].as_array().expect("rows").len(), 1);
    assert_eq!(agent["rows"][0]["label"], "legado/curador");
    assert_eq!(agent["rows"][0]["requests"], 3);

    let operation = usage("operation").await;
    let operations: Vec<(String, u64)> = operation["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| {
            (
                row["key"].as_str().expect("key").to_owned(),
                row["requests"].as_u64().expect("n"),
            )
        })
        .collect();
    assert_eq!(
        operations,
        [
            ("get_context".to_owned(), 2),
            ("search_memories".to_owned(), 1)
        ]
    );

    let channel = usage("channel").await;
    let labels: Vec<&str> = channel["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| row["label"].as_str().expect("label"))
        .collect();
    assert!(
        !labels.is_empty()
            && labels
                .iter()
                .all(|label| ["eng/global", "ops/geral"].contains(label)),
        "{channel}"
    );
    assert_eq!(
        channel["total"], day["total"],
        "o total conta cada requisicao uma vez"
    );

    for query in [
        "group_by=token",
        "since=ontem",
        "since=2020-01-01T00:00:00Z",
        "since=2999-01-01T00:00:00Z",
    ] {
        let (status, body) = context
            .call(
                "GET",
                &format!("/v1/usage?{query}"),
                Some(ADMIN_TOKEN),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
    }
    let (status, _) = context.call("GET", "/v1/usage", Some(&token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn status_shows_uptime_database_backup_webhooks_and_retention() {
    let context = setup().await;
    let (status, body) = context
        .call("GET", "/v1/status", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["uptime_seconds"].is_u64());
    assert!(body["database"]["size_bytes"].as_u64().expect("size") > 0);
    assert_eq!(body["backup"]["status"], "skip");
    assert_eq!(body["backup"]["replica_age_seconds"], Value::Null);
    assert!(body["backup"]["checked_at"].is_string());
    assert_eq!(
        body["redator"]["last_24h"],
        json!({ "calls": 0, "failures": 0, "cost_usd": 0.0 })
    );
    assert_eq!(
        body["webhooks"],
        json!({ "enabled": true, "apps": 0, "pending": 0, "delivered_24h": 0, "failed_24h": 0 })
    );
    assert_eq!(body["retention"], Value::Null);

    context
        .core
        .run_retention(Utc::now())
        .await
        .expect("retention");
    let (_, body) = context
        .call("GET", "/v1/status", Some(ADMIN_TOKEN), None)
        .await;
    assert!(body["retention"]["last_run_at"].is_string(), "{body}");
    assert_eq!(body["retention"]["removed"]["usage_log"], 0);
}

#[tokio::test]
async fn metrics_require_an_admin_read_token_and_label_operations() {
    let context = setup().await;
    let (status, body) = context.call("GET", "/metrics", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let agent = context
        .issue("legado", Some("legado/curador"), "agent", &["read"])
        .await;
    let (status, body) = context.call("GET", "/metrics", Some(&agent), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "SCOPE_REQUIRED");

    let (status, headers, first) = context
        .call_with("GET", "/v1/context", Some(ADMIN_TOKEN), None, &[])
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let etag = headers["etag"].to_str().expect("etag").to_owned();
    let (status, _, _) = context
        .call_with(
            "GET",
            "/v1/context",
            Some(ADMIN_TOKEN),
            None,
            &[("If-None-Match", &etag)],
        )
        .await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    context.call_tool(ADMIN_TOKEN, "whoami", json!({})).await;
    context
        .create_memory(
            ADMIN_TOKEN,
            CHANNEL,
            "Fato para a fila aparecer nas metricas",
        )
        .await;

    let response = context
        .send(
            axum::http::Request::get("/metrics")
                .header("authorization", format!("Bearer {ADMIN_TOKEN}"))
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .expect("type")
            .starts_with("text/plain")
    );
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .expect("body")
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    let has = |needles: &[&str]| {
        text.lines()
            .any(|line| needles.iter().all(|needle| line.contains(needle)))
    };
    assert!(
        has(&[
            "http_requests_total{",
            "operation=\"get_context\"",
            "status=\"304\""
        ]),
        "{text}"
    );
    assert!(
        has(&[
            "http_requests_total{",
            "route=\"/mcp\"",
            "operation=\"whoami\""
        ]),
        "{text}"
    );
    assert!(
        has(&["engramind_auth_failures_total{", "reason=\"missing\""]),
        "{text}"
    );
    assert!(
        has(&["engramind_jobs{", "kind=\"redact\"", "state=\"queued\""]),
        "{text}"
    );
    assert!(has(&["engramind_build_info{", "version=\""]), "{text}");
    assert!(has(&["engramind_database_size_bytes"]), "{text}");
    assert!(
        !text.contains(ADMIN_TOKEN) && !text.contains("legado"),
        "sem segredos nem slugs"
    );
}

#[tokio::test]
async fn retention_removes_only_what_expired() {
    let context = setup().await;
    let old = ids::timestamp(Utc::now() - Duration::days(400));
    let recent = ids::timestamp(Utc::now() - Duration::days(1));
    {
        let db = context.db();
        for at in [&old, &recent] {
            db.execute(
                "INSERT INTO usage_log (at, app_id, token_id, op, channel_ids, memory_ids)
                 VALUES (?1, 'app_x', 'tk_x', 'get_context', '[]', '[]')",
                [at],
            )
            .expect("usage");
            db.execute(
                "INSERT INTO llm_calls (at, purpose, provider, model, duration_ms, outcome)
                 VALUES (?1, 'redact', 'p', 'm', 1, 'success')",
                [at],
            )
            .expect("llm");
            db.execute(
                "INSERT INTO audit_log (at, action, entity) VALUES (?1, 'teste.antigo', 'teste')",
                [at],
            )
            .expect("audit");
            db.execute(
                "INSERT INTO jobs (kind, payload_json, state, run_after, created_at, finished_at)
                 VALUES ('webhook', '{}', 'done', ?1, ?1, ?1)",
                [at],
            )
            .expect("job");
        }
        db.execute(
            "INSERT INTO jobs (kind, payload_json, state, run_after, created_at)
             VALUES ('webhook', '{}', 'queued', ?1, ?1)",
            [&old],
        )
        .expect("queued");
    }
    let audit_before = context.count("SELECT count(*) FROM audit_log");
    let report = context
        .core
        .run_retention(Utc::now())
        .await
        .expect("retention");
    assert_eq!(report.removed.usage_log, 1);
    assert_eq!(report.removed.llm_calls, 1);
    assert_eq!(report.removed.audit_log, 0, "audit_days null guarda tudo");
    assert_eq!(report.removed.jobs, 1, "so o done antigo; o queued fica");
    assert_eq!(
        context.count("SELECT count(*) FROM audit_log"),
        audit_before
    );
    assert_eq!(
        context.count("SELECT count(*) FROM jobs WHERE state = 'queued' AND kind = 'webhook'"),
        1
    );

    let (status, body) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "retention.audit_days": 30 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let report = context
        .core
        .run_retention(Utc::now())
        .await
        .expect("retention");
    assert_eq!(report.removed.audit_log, 1);
    // Rodou há menos de 1 h: a tarefa periódica não roda de novo.
    assert!(context.core.retention_tick().await.expect("tick").is_none());
}
