//! Eventos e webhooks (008): destinatários, assinatura HMAC, retentativa, `skipped`,
//! idempotência e cobertura de toda operação de escrita do catálogo, contra um servidor de
//! webhook local.

use std::time::Duration;

use axum::http::StatusCode;
use chrono::Utc;
use engramind::core::operations::OPERATIONS;
use serde_json::{Value, json};

use crate::support::{
    ADMIN_TOKEN, TestContext,
    llm::{ScriptedLlm, openai},
    webhook::{SECRET, WebhookServer},
};

const CHANNEL: &str = "eng/global";

async fn put_settings(context: &TestContext, settings: Value) {
    let (status, body) = context
        .call("PUT", "/v1/settings", Some(ADMIN_TOKEN), Some(settings))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// Configura o webhook do app (já criado).
async fn hook(context: &TestContext, app: &str, server: &WebhookServer) {
    let (status, body) = context
        .call(
            "PATCH",
            &format!("/v1/apps/{app}"),
            Some(ADMIN_TOKEN),
            Some(json!({ "webhook_url": server.url, "webhook_secret": SECRET })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["webhook"]["url"], server.url.as_str());
    assert_eq!(body["webhook"]["secret"]["configured"], true);
    assert!(
        !body.to_string().contains(SECRET),
        "o segredo nunca volta: {body}"
    );
}

/// Executa as entregas prontas daqui a `ahead`.
async fn deliver(context: &TestContext, ahead: Duration) -> usize {
    let now = Utc::now() + chrono::Duration::from_std(ahead).expect("ahead");
    context.core.run_webhook_jobs(now).await
}

fn webhook_jobs(context: &TestContext) -> i64 {
    context.count("SELECT count(*) FROM jobs WHERE kind = 'webhook'")
}

fn channel_version(context: &TestContext, slug: &str) -> i64 {
    context
        .db()
        .query_row(
            "SELECT version FROM channels WHERE slug = ?1",
            [slug],
            |row| row.get(0),
        )
        .expect("version")
}

/// Rede do §19: `gestor` (token admin), `app-a` com agente assinando o canal, `app-b` com
/// agente assinando outro canal e `app-c` assinante sem webhook.
struct Network {
    context: TestContext,
    gestor: WebhookServer,
    app_a: WebhookServer,
    app_b: WebhookServer,
}

async fn network() -> Network {
    let context = TestContext::new().await;
    context.create_channel(CHANNEL).await;
    context.create_channel("ops/geral").await;
    for app in ["gestor", "app-a", "app-b", "app-c"] {
        context.create_app(app).await;
    }
    context.issue("gestor", None, "admin", &["read"]).await;
    for (app, target) in [
        ("app-a", "eng/*"),
        ("app-b", "ops/geral"),
        ("app-c", CHANNEL),
    ] {
        context.create_agent(app, "dev").await;
        context
            .subscribe(&format!("{app}/dev"), target, "inject", None)
            .await;
    }
    let (gestor, app_a, app_b) = (
        WebhookServer::start().await,
        WebhookServer::start().await,
        WebhookServer::start().await,
    );
    hook(&context, "gestor", &gestor).await;
    hook(&context, "app-a", &app_a).await;
    hook(&context, "app-b", &app_b).await;
    Network {
        context,
        gestor,
        app_a,
        app_b,
    }
}

#[tokio::test]
async fn events_are_signed_and_reach_subscribers_and_admin_apps() {
    let net = network().await;
    let memory = net
        .context
        .create_memory(
            ADMIN_TOKEN,
            CHANNEL,
            "Rode so os testes afetados localmente",
        )
        .await;
    let id = memory["memory"]["id"].as_str().expect("id").to_owned();
    assert_eq!(webhook_jobs(&net.context), 2, "gestor e app-a");
    assert_eq!(deliver(&net.context, Duration::ZERO).await, 2);

    assert!(
        net.app_b.deliveries().is_empty(),
        "app-b nao assina o canal"
    );
    for server in [&net.gestor, &net.app_a] {
        let deliveries = server.deliveries();
        assert_eq!(deliveries.len(), 1);
        let delivery = &deliveries[0];
        assert!(delivery.signature_is_valid(SECRET), "assinatura");
        assert!(!delivery.signature_is_valid("outro-segredo-qualquer-com-32-chars"));
        assert_eq!(delivery.header("content-type"), "application/json");
        assert!(delivery.header("user-agent").starts_with("engramind/"));
        assert_eq!(delivery.header("engramind-event-type"), "memory.created");
        let body = &delivery.body;
        assert_eq!(
            delivery.header("engramind-event-id"),
            body["id"].as_str().expect("id")
        );
        assert!(body["id"].as_str().expect("id").starts_with("evt_"));
        assert_eq!(body["type"], "memory.created");
        assert_eq!(body["channel"], CHANNEL);
        assert_eq!(
            body["channel_version"],
            channel_version(&net.context, CHANNEL)
        );
        assert_eq!(body["memory_ids"], json!([id]));
        assert!(body["at"].is_string());
        let text = String::from_utf8_lossy(&delivery.raw);
        assert!(!text.contains("testes afetados"), "sem texto: {text}");
    }
    let ids: Vec<Value> = [&net.gestor, &net.app_a]
        .iter()
        .map(|server| server.deliveries()[0].body["id"].clone())
        .collect();
    assert_eq!(ids[0], ids[1], "o mesmo evento para os dois apps");

    // Um canal que só app-b assina: app-b e o gestor.
    net.context
        .create_memory(ADMIN_TOKEN, "ops/geral", "Deploy so com o pipeline verde")
        .await;
    deliver(&net.context, Duration::ZERO).await;
    assert_eq!(net.app_b.types(), ["memory.created"]);
    assert_eq!(net.app_a.deliveries().len(), 1);
    assert_eq!(net.gestor.deliveries().len(), 2);

    // Configuração global: só os apps admin.
    put_settings(&net.context, json!({ "memory.text_max": 3000 })).await;
    deliver(&net.context, Duration::ZERO).await;
    let last = net.gestor.deliveries().pop().expect("evento");
    assert_eq!(last.body["type"], "settings.updated");
    assert_eq!(last.body["channel"], Value::Null);
    assert_eq!(last.body["channel_version"], Value::Null);
    assert_eq!(last.body["keys"], json!(["memory.text_max"]));
    assert_eq!(net.app_a.deliveries().len(), 1);
}

#[tokio::test]
async fn failures_retry_with_the_same_event_until_failed_and_retry_job_resends() {
    let net = network().await;
    put_settings(&net.context, json!({ "webhooks.max_attempts": 2 })).await;
    deliver(&net.context, Duration::ZERO).await;
    let before = net.gestor.deliveries().len();

    net.gestor.respond(&[500, 503]);
    net.context
        .create_memory(ADMIN_TOKEN, "ops/geral", "Deploy so com o pipeline verde")
        .await;
    deliver(&net.context, Duration::ZERO).await;
    let (status, jobs) = net
        .context
        .call(
            "GET",
            "/v1/jobs?kind=webhook&state=queued",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let queued = jobs["jobs"].as_array().expect("jobs");
    assert_eq!(queued.len(), 1, "{jobs}");
    assert_eq!(queued[0]["last_error"], "http 500");
    assert!(
        !queued[0]["last_error"]
            .as_str()
            .unwrap()
            .contains("127.0.0.1")
    );

    // Antes do backoff (10 s), nada; depois, a segunda tentativa falha de vez.
    assert_eq!(deliver(&net.context, Duration::from_secs(2)).await, 0);
    assert_eq!(deliver(&net.context, Duration::from_secs(11)).await, 1);
    let (_, failed) = net
        .context
        .call(
            "GET",
            "/v1/jobs?kind=webhook&state=failed",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    let failed = failed["jobs"].as_array().expect("jobs").clone();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["last_error"], "http 503");

    let job = failed[0]["id"].as_i64().expect("id");
    let (status, body) = net
        .context
        .call(
            "POST",
            &format!("/v1/jobs/{job}/retry"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(deliver(&net.context, Duration::ZERO).await, 1);

    let deliveries = &net.gestor.deliveries()[before..];
    assert_eq!(deliveries.len(), 3);
    assert!(
        deliveries
            .iter()
            .all(|delivery| delivery.body == deliveries[0].body
                && delivery.signature_is_valid(SECRET)),
        "mesmo id e mesmo corpo em todas as tentativas"
    );
    let outcome: String = net
        .context
        .db()
        .query_row("SELECT outcome FROM jobs WHERE id = ?1", [job], |row| {
            row.get(0)
        })
        .expect("outcome");
    assert_eq!(outcome, "delivered");
}

#[tokio::test]
async fn removed_disabled_or_switched_off_destinations_are_skipped() {
    let net = network().await;
    deliver(&net.context, Duration::ZERO).await;
    net.context
        .create_memory(
            ADMIN_TOKEN,
            CHANNEL,
            "Rode so os testes afetados localmente",
        )
        .await;
    // Entre o enfileiramento e a entrega, app-a tira a URL e o gestor é desativado.
    let (status, body) = net
        .context
        .call(
            "PATCH",
            "/v1/apps/app-a",
            Some(ADMIN_TOKEN),
            Some(json!({ "webhook_url": null })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["webhook"], Value::Null);
    let (status, _) = net
        .context
        .call(
            "PATCH",
            "/v1/apps/gestor",
            Some(ADMIN_TOKEN),
            Some(json!({ "disabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let before = (net.gestor.deliveries().len(), net.app_a.deliveries().len());
    deliver(&net.context, Duration::ZERO).await;
    assert_eq!(
        (net.gestor.deliveries().len(), net.app_a.deliveries().len()),
        before
    );
    assert_eq!(
        net.context
            .count("SELECT count(*) FROM jobs WHERE kind = 'webhook' AND outcome = 'skipped'"),
        2
    );

    // Desligados, as mutações não enfileiram nada.
    put_settings(&net.context, json!({ "webhooks.enabled": false })).await;
    let jobs = webhook_jobs(&net.context);
    net.context
        .create_memory(ADMIN_TOKEN, "ops/geral", "Deploy so com o pipeline verde")
        .await;
    assert_eq!(webhook_jobs(&net.context), jobs);
}

#[tokio::test]
async fn a_url_requires_a_secret_and_must_be_http_without_credentials() {
    let context = TestContext::new().await;
    context.create_app("gestor").await;
    for body in [
        json!({ "webhook_url": "https://gestor.interno/eventos" }),
        json!({ "webhook_url": "ftp://gestor.interno/eventos", "webhook_secret": SECRET }),
        json!({ "webhook_url": "https://user:senha@gestor.interno", "webhook_secret": SECRET }),
        json!({ "webhook_url": "https://gestor.interno", "webhook_secret": "curto" }),
    ] {
        let (status, response) = context
            .call(
                "PATCH",
                "/v1/apps/gestor",
                Some(ADMIN_TOKEN),
                Some(body.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {response}");
        assert_eq!(response["error"]["code"], "VALIDATION_ERROR");
    }
    let (status, created) = context
        .call(
            "POST",
            "/v1/apps",
            Some(ADMIN_TOKEN),
            Some(json!({ "slug": "outro", "name": "Outro",
                         "webhook_url": "https://outro.interno/eventos", "webhook_secret": SECRET })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["webhook"]["secret"]["configured"], true);
    // Segredo primeiro, URL depois: vale o segredo já gravado.
    let (status, _) = context
        .call(
            "PATCH",
            "/v1/apps/gestor",
            Some(ADMIN_TOKEN),
            Some(json!({ "webhook_secret": SECRET })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = context
        .call(
            "PATCH",
            "/v1/apps/gestor",
            Some(ADMIN_TOKEN),
            Some(json!({ "webhook_url": "http://gestor:9000/eventos" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let audit: String = context
        .db()
        .query_row(
            "SELECT group_concat(coalesce(after_json, ''), ' ') FROM audit_log WHERE entity = 'app'",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    assert!(!audit.contains(SECRET), "auditoria sem o segredo");
    let stored: Vec<u8> = context
        .db()
        .query_row(
            "SELECT webhook_secret_enc FROM apps WHERE slug = 'gestor'",
            [],
            |row| row.get(0),
        )
        .expect("secret");
    assert!(
        !String::from_utf8_lossy(&stored).contains(SECRET),
        "cifrado"
    );
}

#[tokio::test]
async fn moves_emit_one_event_per_channel_and_replays_emit_nothing() {
    let net = network().await;
    net.context.create_channel("eng/outro").await;
    deliver(&net.context, Duration::ZERO).await;
    let memory = net
        .context
        .create_memory(
            ADMIN_TOKEN,
            CHANNEL,
            "Rode so os testes afetados localmente",
        )
        .await;
    let id = memory["memory"]["id"].as_str().expect("id").to_owned();
    deliver(&net.context, Duration::ZERO).await;
    let before = net.gestor.deliveries().len();

    let call = |key: &'static str| {
        let context = &net.context;
        let id = id.clone();
        async move {
            context
                .call_with(
                    "POST",
                    &format!("/v1/memories/{id}/move"),
                    Some(ADMIN_TOKEN),
                    Some(json!({ "to": "eng/outro" })),
                    &[("Idempotency-Key", key)],
                )
                .await
        }
    };
    let (status, _, body) = call("mover-1").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let jobs = webhook_jobs(&net.context);
    let (status, headers, _) = call("mover-1").await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("idempotent-replayed").is_some() || webhook_jobs(&net.context) == jobs);
    assert_eq!(
        webhook_jobs(&net.context),
        jobs,
        "a repeticao nao enfileira"
    );

    deliver(&net.context, Duration::ZERO).await;
    let events: Vec<Value> = net.gestor.deliveries()[before..]
        .iter()
        .map(|delivery| delivery.body.clone())
        .collect();
    assert_eq!(events.len(), 2, "{events:?}");
    let mut channels: Vec<(String, i64)> = events
        .iter()
        .map(|event| {
            assert_eq!(event["type"], "memory.updated");
            assert_eq!(event["memory_ids"], json!([id]));
            (
                event["channel"].as_str().expect("canal").to_owned(),
                event["channel_version"].as_i64().expect("versao"),
            )
        })
        .collect();
    channels.sort();
    assert_eq!(
        channels,
        [
            (CHANNEL.to_owned(), channel_version(&net.context, CHANNEL)),
            (
                "eng/outro".to_owned(),
                channel_version(&net.context, "eng/outro")
            ),
        ]
    );
    assert_ne!(events[0]["id"], events[1]["id"]);
}

#[tokio::test]
async fn subscription_events_name_the_subscription_agent_and_target() {
    let net = network().await;
    deliver(&net.context, Duration::ZERO).await;
    let before = (net.gestor.deliveries().len(), net.app_a.deliveries().len());
    let subscription = net
        .context
        .subscribe("app-a/dev", "ops/geral", "search", None)
        .await;
    let id = subscription["id"].as_str().expect("id");
    let (status, _) = net
        .context
        .call(
            "DELETE",
            &format!("/v1/subscriptions/{id}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    deliver(&net.context, Duration::ZERO).await;
    let events: Vec<Value> = net.app_a.deliveries()[before.1..]
        .iter()
        .map(|delivery| delivery.body.clone())
        .collect();
    assert_eq!(
        events.len(),
        2,
        "o app do agente recebe mesmo depois de sair"
    );
    let agent: String = net
        .context
        .db()
        .query_row(
            "SELECT g.id FROM agents g JOIN apps a ON a.id = g.app_id
             WHERE a.slug = 'app-a' AND g.slug = 'dev'",
            [],
            |row| row.get(0),
        )
        .expect("agent");
    for event in &events {
        assert_eq!(event["type"], "subscription.updated");
        assert_eq!(event["subscription"], id);
        assert_eq!(event["agent"], agent.as_str());
        assert_eq!(event["target"], "ops/geral");
        assert_eq!(event["channel"], "ops/geral");
    }
    assert_eq!(net.gestor.deliveries().len(), before.0 + 2);
    assert_eq!(net.app_b.deliveries().len(), 0, "app-b nao e o dono");
}

#[tokio::test]
async fn the_redator_title_is_a_memory_update_event() {
    let net = network().await;
    let llm = ScriptedLlm::start().await;
    put_settings(
        &net.context,
        json!({
            "redator.provider": "openai_compatible",
            "redator.model": "modelo",
            "redator.base_url": format!("{}/v1", llm.url),
            "redator.timeout_ms": 2000,
        }),
    )
    .await;
    let (status, _) = net
        .context
        .call(
            "PUT",
            "/v1/secrets/redator.api_key",
            Some(ADMIN_TOKEN),
            Some(json!({ "value": "sk-chave-de-teste" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    deliver(&net.context, Duration::ZERO).await;
    let memory = net
        .context
        .create_memory(
            ADMIN_TOKEN,
            CHANNEL,
            "Rode so os testes afetados localmente",
        )
        .await;
    deliver(&net.context, Duration::ZERO).await;
    let before = net.app_a.deliveries().len();
    llm.push(
        200,
        openai(json!({ "title": "Testes afetados", "text": "Rode os testes afetados." })),
    );
    assert_eq!(net.context.core.run_redact_jobs(Utc::now()).await, 1);
    deliver(&net.context, Duration::ZERO).await;
    let events = &net.app_a.deliveries()[before..];
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].body["type"], "memory.updated");
    assert_eq!(
        events[0].body["memory_ids"],
        json!([memory["memory"]["id"]])
    );
}

/// Operações de escrita sem evento (contracts.md): identidades (Q-014), testers, reindexação
/// e reenvio de job.
const WITHOUT_EVENTS: &[&str] = &[
    "create_app",
    "update_app",
    "delete_app",
    "create_agent",
    "update_agent",
    "delete_agent",
    "issue_token",
    "update_token",
    "revoke_token",
    "test_redator",
    "test_embedding",
    "list_embedding_models",
    "reembed_all",
    "retry_job",
];

/// Prepara o alvo da operação e devolve a chamada que deve gerar o evento.
async fn exercise(
    context: &TestContext,
    name: &str,
    n: usize,
) -> (&'static str, String, Option<Value>) {
    let memory = |text: String| async move {
        context.create_memory(ADMIN_TOKEN, CHANNEL, &text).await["memory"]["id"]
            .as_str()
            .expect("id")
            .to_owned()
    };
    match name {
        "create_channel" => (
            "POST",
            "/v1/channels".into(),
            Some(json!({ "slug": format!("eng/c-{n}"), "name": "C" })),
        ),
        "update_channel" => (
            "PATCH",
            format!("/v1/channels/{}", "eng%2Fglobal"),
            Some(json!({ "name": format!("Global {n}") })),
        ),
        "archive_channel" | "restore_channel" | "delete_channel" => {
            let slug = format!("eng/a-{n}");
            context.create_channel(&slug).await;
            let path = format!("/v1/channels/{}", slug.replace('/', "%2F"));
            if name != "archive_channel" {
                let (status, body) = context
                    .call("POST", &format!("{path}/archive"), Some(ADMIN_TOKEN), None)
                    .await;
                assert_eq!(status, StatusCode::OK, "{body}");
            }
            match name {
                "archive_channel" => ("POST", format!("{path}/archive"), None),
                "restore_channel" => ("POST", format!("{path}/restore"), None),
                _ => ("DELETE", path, None),
            }
        }
        "update_channel_settings" => (
            "PUT",
            "/v1/channels/eng%2Fglobal/settings".into(),
            Some(json!({ "memory.text_max": 3000 + n })),
        ),
        "subscribe" => (
            "POST",
            "/v1/subscriptions".into(),
            Some(json!({ "agent": "app-a/dev", "target": format!("s-{n}/*"), "mode": "search" })),
        ),
        "update_subscription" | "unsubscribe" => {
            let created = context
                .subscribe("app-a/dev", &format!("u-{n}/*"), "search", None)
                .await;
            let path = format!("/v1/subscriptions/{}", created["id"].as_str().expect("id"));
            if name == "unsubscribe" {
                ("DELETE", path, None)
            } else {
                ("PATCH", path, Some(json!({ "mode": "inject" })))
            }
        }
        "create_memory" => (
            "POST",
            "/v1/memories".into(),
            Some(json!({ "channel": CHANNEL, "text": format!("memoria de cobertura {n}") })),
        ),
        "import_memories" => (
            "POST",
            "/v1/memories/import".into(),
            Some(json!({ "channel": CHANNEL, "memories": [{ "text": format!("importada {n}") }] })),
        ),
        "update_memory" | "archive_memory" | "restore_memory" | "pin_memory" | "unpin_memory"
        | "move_memory" | "revert_memory" | "delete_memory" => {
            let id = memory(format!("memoria alvo {n}")).await;
            let path = format!("/v1/memories/{id}");
            let prepare = |action: &'static str| {
                let path = path.clone();
                async move {
                    let (status, body) = context
                        .call("POST", &format!("{path}/{action}"), Some(ADMIN_TOKEN), None)
                        .await;
                    assert_eq!(status, StatusCode::OK, "{body}");
                }
            };
            match name {
                "update_memory" => (
                    "PATCH",
                    path,
                    Some(json!({ "text": format!("editada {n}") })),
                ),
                "archive_memory" => ("POST", format!("{path}/archive"), None),
                "restore_memory" => {
                    prepare("archive").await;
                    ("POST", format!("{path}/restore"), None)
                }
                "pin_memory" => ("POST", format!("{path}/pin"), None),
                "unpin_memory" => {
                    prepare("pin").await;
                    ("POST", format!("{path}/unpin"), None)
                }
                "move_memory" => (
                    "POST",
                    format!("{path}/move"),
                    Some(json!({ "to": "ops/geral" })),
                ),
                "revert_memory" => {
                    let (status, body) = context
                        .call(
                            "PATCH",
                            &path,
                            Some(ADMIN_TOKEN),
                            Some(json!({ "text": format!("segunda versao {n}") })),
                        )
                        .await;
                    assert_eq!(status, StatusCode::OK, "{body}");
                    (
                        "POST",
                        format!("{path}/revert"),
                        Some(json!({ "version": 1 })),
                    )
                }
                _ => ("DELETE", path, None),
            }
        }
        "update_settings" => (
            "PUT",
            "/v1/settings".into(),
            Some(json!({ "memory.text_max": 3000 + n })),
        ),
        "set_secret" => (
            "PUT",
            "/v1/secrets/redator.api_key".into(),
            Some(json!({ "value": format!("sk-{n}") })),
        ),
        other => panic!("operacao de escrita sem caso de evento: {other}"),
    }
}

#[tokio::test]
async fn every_write_operation_enqueues_its_event() {
    let net = network().await;
    let mut checked = 0;
    for (n, operation) in OPERATIONS
        .iter()
        .filter(|operation| !operation.read_only && !WITHOUT_EVENTS.contains(&operation.name))
        .enumerate()
    {
        let (method, uri, body) = exercise(&net.context, operation.name, n).await;
        let before = webhook_jobs(&net.context);
        let key = format!("cobertura-{n}");
        let (status, _, response) = net
            .context
            .call_with(
                method,
                &uri,
                Some(ADMIN_TOKEN),
                body,
                &[("Idempotency-Key", &key)],
            )
            .await;
        assert!(
            status.is_success(),
            "{}: {status} {response}",
            operation.name
        );
        assert!(
            webhook_jobs(&net.context) > before,
            "{} nao enfileirou evento",
            operation.name
        );
        checked += 1;
    }
    assert!(checked >= 20, "poucas operacoes de escrita: {checked}");
    for name in WITHOUT_EVENTS {
        assert!(
            OPERATIONS
                .iter()
                .any(|operation| operation.name == *name && !operation.read_only),
            "{name} fora do catalogo de escrita"
        );
    }
}
