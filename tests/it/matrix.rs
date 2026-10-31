//! Matriz tipo × escopo × operação nas três vias (REST, MCP e CLI): cada operação do catálogo
//! é chamada por tokens `agent` e `admin` com as quatro combinações de escopos.

use std::{
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use axum::http::StatusCode;
use engramind::core::{
    authz,
    operations::{OPERATIONS, OperationSpec},
    principal::{AppRef, Principal, ScopeSet, TokenKind},
};
use serde_json::{Value, json};

use crate::support::TestContext;

const SCOPE_SETS: &[&[&str]] = &[
    &["read"],
    &["read", "write"],
    &["read", "delete"],
    &["read", "write", "delete"],
];

static UNIQUE: AtomicUsize = AtomicUsize::new(0);

fn unique() -> usize {
    UNIQUE.fetch_add(1, Ordering::Relaxed)
}

struct Caller {
    kind: TokenKind,
    scopes: &'static [&'static str],
    secret: String,
}

impl Caller {
    fn label(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.scopes.join("+"))
    }

    fn allowed(&self, operation: &OperationSpec) -> bool {
        let principal = Principal {
            token_id: "tk_00000000".to_owned(),
            app: AppRef {
                id: "app_00000000".to_owned(),
                slug: "matrix".to_owned(),
            },
            agent: None,
            kind: self.kind,
            scopes: ScopeSet::parse(&self.scopes.join(",")).expect("scopes"),
        };
        authz::allows(&principal, operation)
    }
}

async fn callers(context: &TestContext) -> Vec<Caller> {
    context.create_app("matrix").await;
    context.create_agent("matrix", "agente").await;
    context.create_channel("matrix/canal").await;
    context
        .subscribe("matrix/agente", "matrix/canal", "inject", None)
        .await;
    let mut callers = Vec::new();
    for kind in [TokenKind::Agent, TokenKind::Admin] {
        for scopes in SCOPE_SETS {
            let agent = (kind == TokenKind::Agent).then_some("matrix/agente");
            let secret = context.issue("matrix", agent, kind.as_str(), scopes).await;
            callers.push(Caller {
                kind,
                scopes,
                secret,
            });
        }
    }
    callers
}

/// REST: método, URI e corpo de cada operação, contra alvos que existem ou que dão 404.
fn rest_call(name: &str) -> (&'static str, String, Option<Value>) {
    let n = unique();
    match name {
        "whoami" => ("GET", "/v1/whoami".into(), None),
        "list_apps" => ("GET", "/v1/apps".into(), None),
        "get_app" => ("GET", "/v1/apps/matrix".into(), None),
        "create_app" => (
            "POST",
            "/v1/apps".into(),
            Some(json!({ "slug": format!("m-{n}"), "name": "M" })),
        ),
        "update_app" => (
            "PATCH",
            "/v1/apps/matrix".into(),
            Some(json!({ "name": "Matrix" })),
        ),
        "delete_app" => ("DELETE", "/v1/apps/nao-existe".into(), None),
        "list_agents" => ("GET", "/v1/agents".into(), None),
        "get_agent" => ("GET", "/v1/agents/matrix%2Fagente".into(), None),
        "create_agent" => (
            "POST",
            "/v1/agents".into(),
            Some(json!({ "app": "matrix", "slug": format!("a-{n}"), "name": "A" })),
        ),
        "update_agent" => (
            "PATCH",
            "/v1/agents/matrix%2Fagente".into(),
            Some(json!({ "name": "Agente" })),
        ),
        "delete_agent" => ("DELETE", "/v1/agents/matrix%2Fnao-existe".into(), None),
        "list_tokens" => ("GET", "/v1/tokens".into(), None),
        "issue_token" => (
            "POST",
            "/v1/tokens".into(),
            Some(json!({ "app": "matrix", "kind": "admin", "scopes": ["read"] })),
        ),
        "update_token" => (
            "PATCH",
            "/v1/tokens/tk_ffffffff".into(),
            Some(json!({ "label": "x" })),
        ),
        "revoke_token" => ("POST", "/v1/tokens/tk_ffffffff/revoke".into(), None),
        "list_channels" => ("GET", "/v1/channels".into(), None),
        "get_channel" => ("GET", "/v1/channels/matrix%2Fcanal".into(), None),
        "create_channel" => (
            "POST",
            "/v1/channels".into(),
            Some(json!({ "slug": format!("m/c-{n}"), "name": "C" })),
        ),
        "update_channel" => (
            "PATCH",
            "/v1/channels/matrix%2Fcanal".into(),
            Some(json!({ "name": "Canal" })),
        ),
        "archive_channel" => ("POST", "/v1/channels/nao%2Fexiste/archive".into(), None),
        "restore_channel" => ("POST", "/v1/channels/nao%2Fexiste/restore".into(), None),
        "delete_channel" => ("DELETE", "/v1/channels/nao%2Fexiste".into(), None),
        "list_subscriptions" => ("GET", "/v1/subscriptions".into(), None),
        "subscribe" => (
            "POST",
            "/v1/subscriptions".into(),
            Some(
                json!({ "agent": "matrix/agente", "target": format!("m-{n}/*"), "mode": "search" }),
            ),
        ),
        "update_subscription" => (
            "PATCH",
            "/v1/subscriptions/sub_ffffffff".into(),
            Some(json!({ "mode": "search" })),
        ),
        "unsubscribe" => ("DELETE", "/v1/subscriptions/sub_ffffffff".into(), None),
        "get_settings" => ("GET", "/v1/settings".into(), None),
        "update_settings" => (
            "PUT",
            "/v1/settings".into(),
            Some(json!({ "limits.max_batch": 100 })),
        ),
        "channel_stats" => ("GET", "/v1/channels/matrix%2Fcanal/stats".into(), None),
        "get_channel_settings" => ("GET", "/v1/channels/matrix%2Fcanal/settings".into(), None),
        "update_channel_settings" => (
            "PUT",
            "/v1/channels/matrix%2Fcanal/settings".into(),
            Some(json!({ "memory.text_max": 4000 })),
        ),
        "get_context" => ("GET", "/v1/context".into(), None),
        "list_memories" => ("GET", "/v1/memories".into(), None),
        "get_memory" => ("GET", "/v1/memories/m_ffffffff".into(), None),
        "search_memories" => (
            "POST",
            "/v1/memories/search".into(),
            Some(json!({ "query": "matriz" })),
        ),
        "memory_history" => ("GET", "/v1/memories/m_ffffffff/history".into(), None),
        "create_memory" => (
            "POST",
            "/v1/memories".into(),
            Some(json!({ "channel": "matrix/canal", "text": format!("memoria da matriz {n}") })),
        ),
        "update_memory" => (
            "PATCH",
            "/v1/memories/m_ffffffff".into(),
            Some(json!({ "category": "NAMING" })),
        ),
        "archive_memory" | "restore_memory" | "pin_memory" | "unpin_memory" => {
            let action = name.trim_end_matches("_memory");
            ("POST", format!("/v1/memories/m_ffffffff/{action}"), None)
        }
        "move_memory" => (
            "POST",
            "/v1/memories/m_ffffffff/move".into(),
            Some(json!({ "to": "matrix/canal" })),
        ),
        "revert_memory" => (
            "POST",
            "/v1/memories/m_ffffffff/revert".into(),
            Some(json!({ "version": 1 })),
        ),
        "delete_memory" => ("DELETE", "/v1/memories/m_ffffffff".into(), None),
        "get_status" => ("GET", "/v1/status".into(), None),
        "get_audit" => ("GET", "/v1/audit".into(), None),
        "get_overview" => ("GET", "/v1/docs/overview".into(), None),
        "list_guides" => ("GET", "/v1/docs/guides".into(), None),
        "get_guide" => ("GET", "/v1/docs/guides/overview".into(), None),
        "search_docs" => ("GET", "/v1/docs/search?q=token".into(), None),
        "list_operations" => ("GET", "/v1/docs/operations".into(), None),
        "get_operation" => ("GET", "/v1/docs/operations/whoami".into(), None),
        "get_schema" => ("GET", "/v1/docs/schemas/WhoAmI".into(), None),
        other => panic!("operacao sem caso na matriz: {other}"),
    }
}

fn mcp_arguments(name: &str) -> Value {
    let n = unique();
    match name {
        "get_app" => json!({ "app": "matrix" }),
        "update_app" => json!({ "app": "matrix", "name": "Matrix" }),
        "create_app" => json!({ "slug": format!("m-{n}"), "name": "M" }),
        "delete_app" => json!({ "app": "nao-existe" }),
        "get_agent" => json!({ "agent": "matrix/agente" }),
        "create_agent" => json!({ "app": "matrix", "slug": format!("a-{n}"), "name": "A" }),
        "update_agent" => json!({ "agent": "matrix/agente", "name": "Agente" }),
        "delete_agent" => json!({ "agent": "matrix/nao-existe" }),
        "issue_token" => json!({ "app": "matrix", "kind": "admin", "scopes": ["read"] }),
        "update_token" => json!({ "token": "tk_ffffffff", "label": "x" }),
        "revoke_token" => json!({ "token": "tk_ffffffff" }),
        "get_channel" => json!({ "channel": "matrix/canal" }),
        "create_channel" => json!({ "slug": format!("m/c-{n}"), "name": "C" }),
        "update_channel" => json!({ "channel": "matrix/canal", "name": "Canal" }),
        "archive_channel" | "restore_channel" | "delete_channel" => {
            json!({ "channel": "nao/existe" })
        }
        "subscribe" => {
            json!({ "agent": "matrix/agente", "target": format!("m-{n}/*"), "mode": "search" })
        }
        "update_subscription" => json!({ "subscription": "sub_ffffffff", "mode": "search" }),
        "unsubscribe" => json!({ "subscription": "sub_ffffffff" }),
        "update_settings" => json!({ "settings": { "limits.max_batch": 100 } }),
        "channel_stats" | "get_channel_settings" => json!({ "channel": "matrix/canal" }),
        "update_channel_settings" => {
            json!({ "channel": "matrix/canal", "settings": { "memory.text_max": 4000 } })
        }
        "get_memory" | "memory_history" | "archive_memory" | "restore_memory" | "pin_memory"
        | "unpin_memory" | "delete_memory" => json!({ "memory": "m_ffffffff" }),
        "search_memories" => json!({ "query": "matriz" }),
        "create_memory" => {
            json!({ "channel": "matrix/canal", "text": format!("memoria da matriz {n}") })
        }
        "update_memory" => json!({ "memory": "m_ffffffff", "category": "NAMING" }),
        "move_memory" => json!({ "memory": "m_ffffffff", "to": "matrix/canal" }),
        "revert_memory" => json!({ "memory": "m_ffffffff", "version": 1 }),
        "get_guide" => json!({ "guide": "overview" }),
        "search_docs" => json!({ "q": "token" }),
        "get_operation" => json!({ "operation": "whoami" }),
        "get_schema" => json!({ "schema": "WhoAmI" }),
        _ => json!({}),
    }
}

fn cli_arguments(name: &str) -> Vec<String> {
    let n = unique();
    let args: Vec<String> = match name {
        "get_app" => vec!["apps".into(), "get".into(), "matrix".into()],
        "create_app" => vec![
            "apps".into(),
            "create".into(),
            "--slug".into(),
            format!("m-{n}"),
            "--name".into(),
            "M".into(),
        ],
        "update_app" => vec![
            "apps".into(),
            "update".into(),
            "matrix".into(),
            "--name".into(),
            "Matrix".into(),
        ],
        "delete_app" => vec!["apps".into(), "delete".into(), "nao-existe".into()],
        "get_agent" => vec!["agents".into(), "get".into(), "matrix/agente".into()],
        "create_agent" => vec![
            "agents".into(),
            "create".into(),
            "--app".into(),
            "matrix".into(),
            "--slug".into(),
            format!("a-{n}"),
            "--name".into(),
            "A".into(),
        ],
        "update_agent" => vec![
            "agents".into(),
            "update".into(),
            "matrix/agente".into(),
            "--name".into(),
            "Agente".into(),
        ],
        "delete_agent" => vec!["agents".into(), "delete".into(), "matrix/nao-existe".into()],
        "issue_token" => vec![
            "tokens".into(),
            "issue".into(),
            "--app".into(),
            "matrix".into(),
            "--kind".into(),
            "admin".into(),
            "--scopes".into(),
            "read".into(),
        ],
        "update_token" => vec![
            "tokens".into(),
            "update".into(),
            "tk_ffffffff".into(),
            "--label".into(),
            "x".into(),
        ],
        "revoke_token" => vec!["tokens".into(), "revoke".into(), "tk_ffffffff".into()],
        "get_channel" => vec!["channels".into(), "get".into(), "matrix/canal".into()],
        "create_channel" => vec![
            "channels".into(),
            "create".into(),
            "--slug".into(),
            format!("m/c-{n}"),
            "--name".into(),
            "C".into(),
        ],
        "update_channel" => vec![
            "channels".into(),
            "update".into(),
            "matrix/canal".into(),
            "--name".into(),
            "Canal".into(),
        ],
        "archive_channel" => vec!["channels".into(), "archive".into(), "nao/existe".into()],
        "restore_channel" => vec!["channels".into(), "restore".into(), "nao/existe".into()],
        "delete_channel" => vec!["channels".into(), "delete".into(), "nao/existe".into()],
        "subscribe" => vec![
            "subscriptions".into(),
            "add".into(),
            "--agent".into(),
            "matrix/agente".into(),
            "--target".into(),
            format!("m-{n}/*"),
            "--mode".into(),
            "search".into(),
        ],
        "update_subscription" => vec![
            "subscriptions".into(),
            "update".into(),
            "sub_ffffffff".into(),
            "--mode".into(),
            "search".into(),
        ],
        "unsubscribe" => vec![
            "subscriptions".into(),
            "remove".into(),
            "sub_ffffffff".into(),
        ],
        "get_settings" => vec!["settings".into(), "get".into()],
        "update_settings" => vec![
            "settings".into(),
            "set".into(),
            "limits.max_batch".into(),
            "100".into(),
        ],
        "channel_stats" => vec!["channels".into(), "stats".into(), "matrix/canal".into()],
        "get_channel_settings" => vec![
            "channels".into(),
            "settings".into(),
            "get".into(),
            "matrix/canal".into(),
        ],
        "update_channel_settings" => vec![
            "channels".into(),
            "settings".into(),
            "set".into(),
            "matrix/canal".into(),
            "memory.text_max".into(),
            "4000".into(),
        ],
        "get_memory" | "memory_history" | "archive_memory" | "restore_memory" | "pin_memory"
        | "unpin_memory" | "delete_memory" => {
            let operation = OPERATIONS
                .iter()
                .find(|operation| operation.name == name)
                .expect("operation");
            let mut args: Vec<String> = operation
                .cli
                .iter()
                .map(|part| (*part).to_owned())
                .collect();
            args.push("m_ffffffff".into());
            args
        }
        "search_memories" => vec!["memories".into(), "search".into(), "matriz".into()],
        "create_memory" => vec![
            "memories".into(),
            "create".into(),
            "--channel".into(),
            "matrix/canal".into(),
            "--text".into(),
            format!("memoria da matriz {n}"),
        ],
        "update_memory" => vec![
            "memories".into(),
            "update".into(),
            "m_ffffffff".into(),
            "--category".into(),
            "NAMING".into(),
        ],
        "move_memory" => vec![
            "memories".into(),
            "move".into(),
            "m_ffffffff".into(),
            "--to".into(),
            "matrix/canal".into(),
        ],
        "revert_memory" => vec![
            "memories".into(),
            "revert".into(),
            "m_ffffffff".into(),
            "--version".into(),
            "1".into(),
        ],
        "get_guide" => vec!["docs".into(), "guide".into(), "overview".into()],
        "search_docs" => vec!["docs".into(), "search".into(), "token".into()],
        "get_operation" => vec!["docs".into(), "operation".into(), "whoami".into()],
        "get_schema" => vec!["docs".into(), "schema".into(), "WhoAmI".into()],
        _ => {
            let operation = OPERATIONS
                .iter()
                .find(|operation| operation.name == name)
                .expect("operation");
            operation
                .cli
                .iter()
                .map(|part| (*part).to_owned())
                .collect()
        }
    };
    args
}

#[tokio::test]
async fn rest_enforces_kind_and_scope_for_every_operation() {
    let context = TestContext::new().await;
    for caller in callers(&context).await {
        for operation in OPERATIONS {
            let (method, uri, body) = rest_call(operation.name);
            let (status, response) = context.call(method, &uri, Some(&caller.secret), body).await;
            let label = format!("{} {}", caller.label(), operation.name);
            assert_ne!(status, StatusCode::UNAUTHORIZED, "{label}: {response}");
            if caller.allowed(operation) {
                assert_ne!(status, StatusCode::FORBIDDEN, "{label}: {response}");
                assert!(
                    status.is_success() || status == StatusCode::NOT_FOUND,
                    "{label}: {status} {response}"
                );
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{label}: {response}");
                assert_eq!(response["error"]["code"], "SCOPE_REQUIRED", "{label}");
            }
        }
    }
}

#[tokio::test]
async fn mcp_enforces_kind_and_scope_for_every_operation() {
    let context = TestContext::new().await;
    for caller in callers(&context).await {
        for operation in OPERATIONS {
            let response = context
                .call_tool(
                    &caller.secret,
                    operation.name,
                    mcp_arguments(operation.name),
                )
                .await;
            let result = &response["result"];
            let code = result["structuredContent"]["error"]["code"].as_str();
            let label = format!("{} {}", caller.label(), operation.name);
            if caller.allowed(operation) {
                assert!(
                    result["isError"] == false
                        || code.is_some_and(|code| code.ends_with("_NOT_FOUND")),
                    "{label}: {response}"
                );
            } else {
                assert_eq!(result["isError"], true, "{label}: {response}");
                assert_eq!(code, Some("SCOPE_REQUIRED"), "{label}");
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_enforces_kind_and_scope_for_every_operation() {
    let context = TestContext::new().await;
    let url = context.serve().await;
    let home = context.dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let bin = std::env::var("NEXTEST_BIN_EXE_engramind")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_engramind").to_owned());
    for caller in callers(&context).await {
        let mut runs = Vec::new();
        for operation in OPERATIONS {
            let args = cli_arguments(operation.name);
            let (bin, url, home, secret) = (
                bin.clone(),
                url.clone(),
                home.clone(),
                caller.secret.clone(),
            );
            runs.push((
                operation,
                tokio::task::spawn_blocking(move || {
                    Command::new(bin)
                        .args(&args)
                        .arg("--json")
                        .env_clear()
                        .env("HOME", &home)
                        .env("XDG_CONFIG_HOME", &home)
                        .env("ENGRAMIND_URL", url)
                        .env("ENGRAMIND_TOKEN", secret)
                        .output()
                        .expect("run engramind")
                }),
            ));
        }
        for (operation, run) in runs {
            let output = run.await.expect("join");
            let code = output.status.code().expect("exit code");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let label = format!("{} {}", caller.label(), operation.name);
            if caller.allowed(operation) {
                assert!(code == 0 || code == 5, "{label}: saida {code}: {stdout}");
            } else {
                assert_eq!(code, 4, "{label}: {stdout}");
                let envelope: Value = serde_json::from_str(&stdout).expect("envelope JSON");
                assert_eq!(envelope["error"]["code"], "SCOPE_REQUIRED", "{label}");
            }
        }
    }
}
