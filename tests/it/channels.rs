//! Canais, assinaturas e acesso efetivo pelas três vias (etapa 003).

use std::process::{Command, Output};

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::support::{ADMIN_TOKEN, TestContext};

fn slugs(list: &Value) -> Vec<String> {
    list["channels"]
        .as_array()
        .expect("channels")
        .iter()
        .map(|channel| channel["slug"].as_str().expect("slug").to_owned())
        .collect()
}

async fn agent_token(context: &TestContext, agent: &str, scopes: &[&str]) -> String {
    let app = agent.split('/').next().expect("app");
    context.issue(app, Some(agent), "agent", scopes).await
}

#[tokio::test]
async fn channels_have_a_full_lifecycle_with_version_and_audit() {
    let context = TestContext::new().await;
    let created = context.create_channel("eng/global").await;
    assert_eq!(created["version"], 1);
    assert!(created["id"].as_str().expect("id").starts_with("ch_"));
    assert!(created.get("reach").is_none());

    let (status, body) = context
        .call(
            "POST",
            "/v1/channels",
            Some(ADMIN_TOKEN),
            Some(json!({ "slug": "eng/global", "name": "x" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "SLUG_ALREADY_EXISTS");
    let (status, body) = context
        .call(
            "POST",
            "/v1/channels",
            Some(ADMIN_TOKEN),
            Some(json!({ "slug": "eng/*", "name": "x" })),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("VALIDATION_ERROR"))
    );
    let (status, body) = context
        .call(
            "POST",
            "/v1/channels",
            Some(ADMIN_TOKEN),
            Some(json!({ "slug": "eng/x", "name": "x", "priority": 1001 })),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("VALIDATION_ERROR"))
    );

    let (status, updated) = context
        .call(
            "PATCH",
            "/v1/channels/eng%2Fglobal",
            Some(ADMIN_TOKEN),
            Some(json!({ "priority": -5, "description": "regras" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(
        (updated["version"].as_i64(), updated["priority"].as_i64()),
        (Some(2), Some(-5))
    );
    let (status, body) = context
        .call(
            "PATCH",
            "/v1/channels/eng%2Fglobal",
            Some(ADMIN_TOKEN),
            Some(json!({ "slug": "outro" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "slug imutavel: {body}");

    let id = created["id"].as_str().expect("id");
    let (status, by_id) = context
        .call(
            "GET",
            &format!("/v1/channels/{id}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(by_id["slug"], "eng/global");
    assert_eq!(by_id["reach"]["mode"], "inject");
    assert_eq!(by_id["reach"]["access"], json!(["read", "write", "delete"]));
    let (status, body) = context
        .call("GET", "/v1/channels/eng%2Fnada", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("CHANNEL_NOT_FOUND"))
    );

    let (status, body) = context
        .call(
            "DELETE",
            "/v1/channels/eng%2Fglobal",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("CHANNEL_NOT_ARCHIVED"))
    );

    let (status, archived) = context
        .call(
            "POST",
            "/v1/channels/eng%2Fglobal/archive",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(archived["archived_at"].is_string());
    assert_eq!(archived["version"], 3);
    let (_, again) = context
        .call(
            "POST",
            "/v1/channels/eng%2Fglobal/archive",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(again["version"], 3, "arquivar de novo nao muda nada");

    let (_, list) = context
        .call("GET", "/v1/channels", Some(ADMIN_TOKEN), None)
        .await;
    assert!(slugs(&list).is_empty());
    let (_, list) = context
        .call(
            "GET",
            "/v1/channels?include_archived=true",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(slugs(&list), ["eng/global"]);
    let (status, _) = context
        .call("GET", "/v1/channels/eng%2Fglobal", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "arquivado continua legivel quando citado"
    );

    let (status, restored) = context
        .call(
            "POST",
            "/v1/channels/eng%2Fglobal/restore",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(restored["archived_at"].is_null());
    assert_eq!(restored["version"], 4);

    let (_, audit) = context
        .call(
            "GET",
            &format!("/v1/audit?entity=channel&entity_id={id}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    let actions: Vec<&str> = audit["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| event["action"].as_str().expect("action"))
        .collect();
    assert_eq!(
        actions,
        [
            "channel.restore",
            "channel.archive",
            "channel.update",
            "channel.create"
        ]
    );
}

#[tokio::test]
async fn create_channel_is_idempotent_and_lists_are_paginated_by_slug() {
    let context = TestContext::new().await;
    let body = json!({ "slug": "b/um", "name": "B" });
    let headers = [("Idempotency-Key", "canal-b")];
    let (first, _, created) = context
        .call_with(
            "POST",
            "/v1/channels",
            Some(ADMIN_TOKEN),
            Some(body.clone()),
            &headers,
        )
        .await;
    let (second, _, replayed) = context
        .call_with(
            "POST",
            "/v1/channels",
            Some(ADMIN_TOKEN),
            Some(body),
            &headers,
        )
        .await;
    assert_eq!((first, second), (StatusCode::CREATED, StatusCode::CREATED));
    assert_eq!(created, replayed);
    context.create_channel("a/um").await;
    context.create_channel("c/um").await;
    let (_, page) = context
        .call("GET", "/v1/channels?limit=2", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(slugs(&page), ["a/um", "b/um"]);
    let cursor = page["next_cursor"].as_str().expect("cursor");
    let (_, rest) = context
        .call(
            "GET",
            &format!("/v1/channels?limit=2&cursor={cursor}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(slugs(&rest), ["c/um"]);
    assert!(rest["next_cursor"].is_null());
}

#[tokio::test]
async fn subscriptions_validate_targets_and_reject_duplicates() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_agent("app-a", "dev").await;
    context.create_channel("eng/global").await;

    let exact = context
        .subscribe("app-a/dev", "eng/global", "inject", Some(&["read"]))
        .await;
    assert_eq!(exact["is_prefix"], false);
    assert_eq!(exact["reaches"], json!(["eng/global"]));
    assert_eq!(exact["agent"]["slug"], "app-a/dev");
    let prefix = context
        .subscribe("app-a/dev", "eng/projeto-*", "search", None)
        .await;
    assert_eq!(prefix["target"], "eng/projeto-*");
    assert_eq!(prefix["is_prefix"], true);
    assert_eq!(prefix["access"], Value::Null);
    assert_eq!(
        prefix["reaches"],
        json!([]),
        "prefixo vale para canais futuros"
    );

    let cases = [
        (
            json!({ "agent": "app-a/dev", "target": "eng/global", "mode": "search" }),
            StatusCode::CONFLICT,
            "SUBSCRIPTION_EXISTS",
        ),
        (
            json!({ "agent": "app-a/dev", "target": "eng/nada", "mode": "inject" }),
            StatusCode::NOT_FOUND,
            "CHANNEL_NOT_FOUND",
        ),
        (
            json!({ "agent": "app-a/ninguem", "target": "eng/global", "mode": "inject" }),
            StatusCode::NOT_FOUND,
            "AGENT_NOT_FOUND",
        ),
        (
            json!({ "agent": "app-a/dev", "target": "*", "mode": "inject" }),
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
        ),
        (
            json!({ "agent": "app-a/dev", "target": "eng*", "mode": "inject" }),
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
        ),
        (
            json!({ "agent": "app-a/dev", "target": "eng/x", "mode": "todos" }),
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
        ),
        (
            json!({ "agent": "app-a/dev", "target": "eng/global", "mode": "inject", "access": ["write"] }),
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
        ),
    ];
    for (body, status, code) in cases {
        let (got, response) = context
            .call(
                "POST",
                "/v1/subscriptions",
                Some(ADMIN_TOKEN),
                Some(body.clone()),
            )
            .await;
        assert_eq!(
            (got, response["error"]["code"].as_str()),
            (status, Some(code)),
            "{body}"
        );
    }

    let later = context.create_channel("eng/projeto-atlas").await;
    assert_eq!(later["slug"], "eng/projeto-atlas");
    let id = prefix["id"].as_str().expect("id");
    let (status, updated) = context
        .call(
            "PATCH",
            &format!("/v1/subscriptions/{id}"),
            Some(ADMIN_TOKEN),
            Some(json!({ "mode": "inject", "access": ["read"], "priority": 7 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["reaches"], json!(["eng/projeto-atlas"]));
    assert_eq!(
        (updated["mode"].as_str(), updated["priority"].as_i64()),
        (Some("inject"), Some(7))
    );
    let (_, reset) = context
        .call(
            "PATCH",
            &format!("/v1/subscriptions/{id}"),
            Some(ADMIN_TOKEN),
            Some(json!({ "access": null, "priority": null })),
        )
        .await;
    assert_eq!(
        (reset["access"].clone(), reset["priority"].clone()),
        (Value::Null, Value::Null)
    );
    let (status, body) = context
        .call(
            "PATCH",
            &format!("/v1/subscriptions/{id}"),
            Some(ADMIN_TOKEN),
            Some(json!({ "target": "x/*" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "alvo imutavel: {body}");

    let (_, filtered) = context
        .call(
            "GET",
            "/v1/subscriptions?channel=eng%2Fprojeto-atlas",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(filtered["subscriptions"].as_array().expect("list").len(), 1);

    let (status, removed) = context
        .call(
            "DELETE",
            &format!("/v1/subscriptions/{id}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["id"], id);
    let (status, body) = context
        .call(
            "DELETE",
            &format!("/v1/subscriptions/{id}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("SUBSCRIPTION_NOT_FOUND"))
    );
}

#[tokio::test]
async fn access_changes_apply_on_the_next_request() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_agent("app-a", "dev").await;
    context.create_channel("eng/global").await;
    let token = agent_token(&context, "app-a/dev", &["read", "write"]).await;

    let (status, body) = context
        .call("GET", "/v1/channels/eng%2Fglobal", Some(&token), None)
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("CHANNEL_NOT_SUBSCRIBED"))
    );
    assert_eq!(body["error"]["details"]["channel"], "eng/global");

    let subscription = context
        .subscribe("app-a/dev", "eng/*", "inject", Some(&["read"]))
        .await;
    let (status, channel) = context
        .call("GET", "/v1/channels/eng%2Fglobal", Some(&token), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{channel}");
    assert_eq!(
        channel["reach"]["access"],
        json!(["read"]),
        "a assinatura restringe"
    );
    assert_eq!(channel["reach"]["target"], "eng/*");

    context.create_channel("eng/novo").await;
    let (_, list) = context
        .call("GET", "/v1/channels", Some(&token), None)
        .await;
    assert_eq!(
        slugs(&list),
        ["eng/global", "eng/novo"],
        "prefixo alcanca canal novo"
    );

    let id = subscription["id"].as_str().expect("id");
    let (status, _) = context
        .call(
            "DELETE",
            &format!("/v1/subscriptions/{id}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = context
        .call("GET", "/v1/channels", Some(&token), None)
        .await;
    assert!(slugs(&list).is_empty(), "o cache nao serve acesso revogado");
    let (status, _) = context
        .call("GET", "/v1/channels/eng%2Fglobal", Some(&token), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn agents_see_only_their_own_subscriptions_and_cannot_manage_the_network() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_agent("app-a", "dev").await;
    context.create_agent("app-a", "outro").await;
    context.create_channel("eng/global").await;
    context
        .subscribe("app-a/dev", "eng/global", "inject", None)
        .await;
    context
        .subscribe("app-a/outro", "eng/global", "inject", None)
        .await;
    let token = agent_token(&context, "app-a/dev", &["read", "write", "delete"]).await;

    let (_, own) = context
        .call("GET", "/v1/subscriptions", Some(&token), None)
        .await;
    let agents: Vec<&str> = own["subscriptions"]
        .as_array()
        .expect("list")
        .iter()
        .map(|subscription| subscription["agent"]["slug"].as_str().expect("agent"))
        .collect();
    assert_eq!(agents, ["app-a/dev"]);
    let (_, other) = context
        .call(
            "GET",
            "/v1/subscriptions?agent=app-a%2Foutro",
            Some(&token),
            None,
        )
        .await;
    assert_eq!(other["subscriptions"], json!([]));
    let (_, all) = context
        .call("GET", "/v1/subscriptions", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(all["subscriptions"].as_array().expect("list").len(), 2);

    for (method, uri, body) in [
        (
            "POST",
            "/v1/channels",
            Some(json!({ "slug": "x/y", "name": "x" })),
        ),
        (
            "POST",
            "/v1/subscriptions",
            Some(json!({ "agent": "app-a/dev", "target": "x/*", "mode": "inject" })),
        ),
        ("POST", "/v1/channels/eng%2Fglobal/archive", None),
    ] {
        let (status, response) = context.call(method, uri, Some(&token), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(response["error"]["code"], "SCOPE_REQUIRED");
    }
}

#[tokio::test]
async fn deleting_agents_apps_and_channels_removes_their_subscriptions() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_agent("app-a", "dev").await;
    context.create_agent("app-a", "rev").await;
    context.create_app("app-b").await;
    context.create_agent("app-b", "cli").await;
    context.create_channel("eng/global").await;
    context.create_channel("eng/legado").await;
    context
        .subscribe("app-a/dev", "eng/global", "inject", None)
        .await;
    context
        .subscribe("app-a/dev", "eng/*", "search", None)
        .await;
    context
        .subscribe("app-a/rev", "eng/legado", "inject", None)
        .await;
    context
        .subscribe("app-a/rev", "eng/*", "inject", None)
        .await;
    context
        .subscribe("app-b/cli", "eng/global", "inject", None)
        .await;

    assert_eq!(subscription_count(&context).await, 5);

    let (status, _) = context
        .call("DELETE", "/v1/agents/app-a%2Fdev", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subscription_count(&context).await, 3);
    let (_, audit) = context
        .call(
            "GET",
            "/v1/audit?action=agent.delete",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(audit["events"][0]["before"]["subscriptions_removed"], 2);

    context
        .call(
            "POST",
            "/v1/channels/eng%2Flegado/archive",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    let (status, _) = context
        .call(
            "DELETE",
            "/v1/channels/eng%2Flegado",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        subscription_count(&context).await,
        2,
        "so a exata do canal sai; o prefixo fica"
    );
    let (_, audit) = context
        .call(
            "GET",
            "/v1/audit?action=channel.delete",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(audit["events"][0]["before"]["subscriptions_removed"], 1);

    let (status, _) = context
        .call("DELETE", "/v1/apps/app-b", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subscription_count(&context).await, 1);
    let (_, audit) = context
        .call(
            "GET",
            "/v1/audit?action=app.delete",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(audit["events"][0]["before"]["subscriptions_removed"], 1);
}

async fn subscription_count(context: &TestContext) -> usize {
    let (_, list) = context
        .call("GET", "/v1/subscriptions", Some(ADMIN_TOKEN), None)
        .await;
    list["subscriptions"].as_array().expect("list").len()
}

// ---------- cenário do §19 do desenho, montado pela CLI ----------

fn bin() -> String {
    std::env::var("NEXTEST_BIN_EXE_engramind")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_engramind").to_owned())
}

async fn cli(
    context: &TestContext,
    url: &str,
    token: &str,
    channels: Option<&str>,
    args: &[&str],
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
        .env("ENGRAMIND_TOKEN", token);
    if let Some(channels) = channels {
        command.env("ENGRAMIND_CHANNELS", channels);
    }
    tokio::task::spawn_blocking(move || command.output().expect("run engramind"))
        .await
        .expect("join")
}

async fn cli_ok(context: &TestContext, url: &str, args: &[&str]) -> Value {
    let output = cli(context, url, ADMIN_TOKEN, None, args).await;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{args:?}: {stdout}");
    serde_json::from_str(&stdout).expect("json")
}

/// `(canal, modo, acesso)` vistos pelo agente, em ordem de slug.
fn reaches(list: &Value) -> Vec<(String, String, String)> {
    list["channels"]
        .as_array()
        .expect("channels")
        .iter()
        .map(|channel| {
            let access: Vec<&str> = channel["reach"]["access"]
                .as_array()
                .expect("access")
                .iter()
                .map(|scope| scope.as_str().expect("scope"))
                .collect();
            (
                channel["slug"].as_str().expect("slug").to_owned(),
                channel["reach"]["mode"].as_str().expect("mode").to_owned(),
                access.join("+"),
            )
        })
        .collect()
}

fn expected(rows: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
    rows.iter()
        .map(|(slug, mode, access)| ((*slug).to_owned(), (*mode).to_owned(), (*access).to_owned()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_design_example_is_built_by_the_cli_and_seen_alike_in_the_three_ways() {
    let context = TestContext::new().await;
    let url = context.serve().await;
    for app in ["app-a", "app-b"] {
        cli_ok(
            &context,
            &url,
            &["apps", "create", "--slug", app, "--name", app],
        )
        .await;
    }
    for agent in ["assistente", "desenvolvedor", "revisor", "curador"] {
        cli_ok(
            &context,
            &url,
            &[
                "agents", "create", "--app", "app-a", "--slug", agent, "--name", agent,
            ],
        )
        .await;
    }
    cli_ok(
        &context,
        &url,
        &[
            "agents",
            "create",
            "--app",
            "app-b",
            "--slug",
            "agente-cli",
            "--name",
            "cli",
        ],
    )
    .await;
    for channel in [
        "assistente/geral",
        "eng/global",
        "eng/projeto-atlas",
        "eng/projeto-boreal",
        "review/revisor",
    ] {
        cli_ok(
            &context,
            &url,
            &["channels", "create", "--slug", channel, "--name", channel],
        )
        .await;
    }
    let subscriptions = [
        (
            "app-a/assistente",
            "assistente/geral",
            "inject",
            "read,write",
        ),
        ("app-a/desenvolvedor", "eng/global", "inject", "read"),
        ("app-a/revisor", "eng/global", "inject", "read"),
        ("app-a/curador", "eng/global", "manage", "read,write"),
        ("app-a/desenvolvedor", "eng/projeto-*", "inject", "read"),
        ("app-a/curador", "eng/projeto-*", "manage", "read,write"),
        ("app-a/revisor", "review/revisor", "inject", "read"),
        ("app-a/curador", "review/revisor", "manage", "read,write"),
        ("app-b/agente-cli", "eng/global", "inject", "read,write"),
    ];
    for (agent, target, mode, access) in subscriptions {
        let created = cli_ok(
            &context,
            &url,
            &[
                "subscriptions",
                "add",
                "--agent",
                agent,
                "--target",
                target,
                "--mode",
                mode,
                "--access",
                access,
            ],
        )
        .await;
        assert_eq!(created["target"], target);
    }

    let mut tokens = Vec::new();
    for agent in [
        "app-a/assistente",
        "app-a/desenvolvedor",
        "app-a/revisor",
        "app-a/curador",
        "app-b/agente-cli",
    ] {
        tokens.push((
            agent,
            agent_token(&context, agent, &["read", "write"]).await,
        ));
    }
    let views = [
        expected(&[("assistente/geral", "inject", "read+write")]),
        expected(&[
            ("eng/global", "inject", "read"),
            ("eng/projeto-atlas", "inject", "read"),
            ("eng/projeto-boreal", "inject", "read"),
        ]),
        expected(&[
            ("eng/global", "inject", "read"),
            ("review/revisor", "inject", "read"),
        ]),
        expected(&[
            ("eng/global", "manage", "read+write"),
            ("eng/projeto-atlas", "manage", "read+write"),
            ("eng/projeto-boreal", "manage", "read+write"),
            ("review/revisor", "manage", "read+write"),
        ]),
        expected(&[("eng/global", "inject", "read+write")]),
    ];
    for ((agent, token), view) in tokens.iter().zip(views) {
        let (_, rest) = context.call("GET", "/v1/channels", Some(token), None).await;
        assert_eq!(reaches(&rest), view, "REST {agent}");
        let mcp = context.call_tool(token, "list_channels", json!({})).await;
        assert_eq!(
            reaches(&mcp["result"]["structuredContent"]),
            view,
            "MCP {agent}"
        );
        let output = cli(&context, &url, token, None, &["channels", "list"]).await;
        let stdout: Value = serde_json::from_slice(&output.stdout).expect("json");
        assert_eq!(reaches(&stdout), view, "CLI {agent}");
    }

    // O desenvolvedor numa sessão do projeto atlas: cabeçalho da conexão + `channels`.
    let developer = &tokens[1].1;
    let session = "eng/global,eng/projeto-atlas";
    let atlas = expected(&[
        ("eng/global", "inject", "read"),
        ("eng/projeto-atlas", "inject", "read"),
    ]);
    let (_, _, rest) = context
        .call_with(
            "GET",
            "/v1/channels",
            Some(developer),
            None,
            &[("Engramind-Channels", session)],
        )
        .await;
    assert_eq!(reaches(&rest), atlas);
    let (_, mcp) = context
        .rpc_with(
            developer,
            "tools/call",
            json!({ "name": "list_channels", "arguments": {} }),
            &[("Engramind-Channels", session)],
        )
        .await;
    assert_eq!(reaches(&mcp["result"]["structuredContent"]), atlas);
    let output = cli(
        &context,
        &url,
        developer,
        Some(session),
        &["channels", "list"],
    )
    .await;
    assert_eq!(
        reaches(&serde_json::from_slice(&output.stdout).expect("json")),
        atlas
    );

    // Interseção: o parâmetro não escapa do cabeçalho.
    let (_, _, narrowed) = context
        .call_with(
            "GET",
            "/v1/channels?channels=eng/projeto-boreal,eng/global",
            Some(developer),
            None,
            &[("Engramind-Channels", session)],
        )
        .await;
    assert_eq!(
        reaches(&narrowed),
        expected(&[("eng/global", "inject", "read")])
    );
    let mcp = context
        .call_tool(
            developer,
            "list_channels",
            json!({ "channels": "eng/projeto-*" }),
        )
        .await;
    assert_eq!(
        reaches(&mcp["result"]["structuredContent"]),
        expected(&[
            ("eng/projeto-atlas", "inject", "read"),
            ("eng/projeto-boreal", "inject", "read")
        ])
    );

    // Canal fora da sessão ou sem assinatura: CHANNEL_NOT_SUBSCRIBED nas três vias.
    let (status, _, body) = context
        .call_with(
            "GET",
            "/v1/channels/eng%2Fprojeto-boreal",
            Some(developer),
            None,
            &[("Engramind-Channels", session)],
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("CHANNEL_NOT_SUBSCRIBED"))
    );
    let (_, mcp) = context
        .rpc_with(
            developer,
            "tools/call",
            json!({ "name": "get_channel", "arguments": { "channel": "eng/projeto-boreal" } }),
            &[("Engramind-Channels", session)],
        )
        .await;
    assert_eq!(mcp["result"]["isError"], true);
    assert_eq!(
        mcp["result"]["structuredContent"]["error"]["code"],
        "CHANNEL_NOT_SUBSCRIBED"
    );
    let output = cli(
        &context,
        &url,
        developer,
        None,
        &["channels", "get", "review/revisor"],
    )
    .await;
    assert_eq!(output.status.code(), Some(4));
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(envelope["error"]["code"], "CHANNEL_NOT_SUBSCRIBED");
    assert_eq!(envelope["error"]["details"]["channel"], "review/revisor");

    // Cabeçalho inválido no MCP vira VALIDATION_ERROR com isError.
    let (_, mcp) = context
        .rpc_with(
            developer,
            "tools/call",
            json!({ "name": "list_channels", "arguments": {} }),
            &[("Engramind-Channels", "eng*")],
        )
        .await;
    assert_eq!(
        mcp["result"]["structuredContent"]["error"]["code"],
        "VALIDATION_ERROR"
    );
}
