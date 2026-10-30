//! Identidades, tokens, auditoria, idempotência, rate limit e configurações pelo REST.

use axum::http::{StatusCode, header};
use engramind::{
    config::Secret,
    core::bootstrap::{BootstrapError, BootstrapOutcome},
    store::Store,
};
use serde_json::{Value, json};

use crate::support::{ADMIN_TOKEN, TestContext};

async fn whoami(context: &TestContext, token: &str) -> (StatusCode, Value) {
    context.call("GET", "/v1/whoami", Some(token), None).await
}

#[tokio::test]
async fn bootstrap_runs_once_and_validates_the_format() {
    let context = TestContext::new().await;
    let (status, body) = whoami(&context, ADMIN_TOKEN).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["token_id"], "tk_00000001");
    assert_eq!(body["kind"], "admin");
    assert_eq!(body["scopes"], json!(["read", "write", "delete"]));
    assert_eq!(body["app"]["slug"], "bootstrap");

    let other = Secret::new("eng.tk_00000002.outro-segredo-de-bootstrap-com-32-caracteres".into());
    assert_eq!(
        context.core.bootstrap(Some(&other)).await.expect("again"),
        BootstrapOutcome::Ignored
    );
    assert_eq!(
        whoami(&context, other.expose()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        context.core.bootstrap(None).await.expect("none"),
        BootstrapOutcome::NotConfigured
    );

    let empty = TestContext::unmigrated();
    empty.store.migrate().await.expect("migrate");
    assert_eq!(
        empty.core.bootstrap(None).await.expect("missing"),
        BootstrapOutcome::MissingWithEmptyDatabase
    );
    let invalid = Secret::new("eng.00000000-0000-4000-8000-000000000001.segredo".into());
    assert!(matches!(
        empty.core.bootstrap(Some(&invalid)).await,
        Err(BootstrapError::InvalidFormat)
    ));
    assert_eq!(empty.store.count_tokens().await.expect("count"), 0);
}

#[tokio::test]
async fn a_manager_provisions_an_app_an_agent_and_a_working_agent_token() {
    let context = TestContext::new().await;
    let app = context.create_app("app-a").await;
    assert!(app["id"].as_str().expect("id").starts_with("app_"));
    let agent = context.create_agent("app-a", "curador").await;
    assert_eq!(agent["qualified_slug"], "app-a/curador");
    assert_eq!(agent["app"]["id"], app["id"]);

    let (status, issued) = context
        .call(
            "POST",
            "/v1/tokens",
            Some(ADMIN_TOKEN),
            Some(json!({ "agent": "app-a/curador", "kind": "agent", "scopes": ["read", "write"], "label": "curador" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{issued}");
    let secret = issued["secret"].as_str().expect("secret");
    let token_id = issued["token"]["id"].as_str().expect("id");
    assert!(secret.starts_with(&format!("eng.{token_id}.")));
    assert_eq!(issued["token"]["prefix"], format!("eng.{token_id}"));
    assert_eq!(issued["token"]["app"]["slug"], "app-a");

    let (status, me) = whoami(&context, secret).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["kind"], "agent");
    assert_eq!(me["agent"]["slug"], "app-a/curador");
    let operations: Vec<&str> = me["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(operations.contains(&"whoami") && operations.contains(&"get_guide"));
    assert!(!operations.contains(&"list_apps"));

    let (status, listed) = context
        .call(
            "GET",
            "/v1/tokens?agent=app-a%2Fcurador",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let tokens = listed["tokens"].as_array().expect("tokens");
    assert_eq!(tokens.len(), 1);
    assert!(
        !listed
            .to_string()
            .contains(secret.rsplit('.').next().expect("segredo"))
    );

    for uri in [
        "/v1/agents/app-a%2Fcurador",
        &format!("/v1/agents/{}", agent["id"].as_str().expect("id")),
    ] {
        let (status, found) = context.call("GET", uri, Some(ADMIN_TOKEN), None).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {found}");
        assert_eq!(found["id"], agent["id"]);
    }
}

#[tokio::test]
async fn every_identity_failure_is_auth_invalid() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_agent("app-a", "curador").await;
    let valid = context
        .issue("app-a", Some("app-a/curador"), "agent", &["read"])
        .await;
    let id = valid.split('.').nth(1).expect("id").to_owned();
    let wrong_secret = format!("eng.{id}.{}", "x".repeat(43));
    for token in [
        None,
        Some("eng.invalido"),
        Some("eng.tk_ffffffff.segredo-qualquer-com-mais-de-32-caracteres"),
        Some(wrong_secret.as_str()),
        Some("Basic abc"),
    ] {
        let (status, headers, body) = context
            .call_with("GET", "/v1/whoami", token, None, &[])
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{token:?}");
        assert_eq!(body["error"]["code"], "AUTH_INVALID");
        assert_eq!(headers[header::WWW_AUTHENTICATE], "Bearer");
    }

    // App desativado, depois reativado.
    let (status, _) = context
        .call(
            "PATCH",
            "/v1/apps/app-a",
            Some(ADMIN_TOKEN),
            Some(json!({ "disabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(whoami(&context, &valid).await.0, StatusCode::UNAUTHORIZED);
    context
        .call(
            "PATCH",
            "/v1/apps/app-a",
            Some(ADMIN_TOKEN),
            Some(json!({ "disabled": false })),
        )
        .await;
    assert_eq!(whoami(&context, &valid).await.0, StatusCode::OK);

    // Agente desativado.
    context
        .call(
            "PATCH",
            "/v1/agents/app-a%2Fcurador",
            Some(ADMIN_TOKEN),
            Some(json!({ "disabled": true })),
        )
        .await;
    assert_eq!(whoami(&context, &valid).await.0, StatusCode::UNAUTHORIZED);
    context
        .call(
            "PATCH",
            "/v1/agents/app-a%2Fcurador",
            Some(ADMIN_TOKEN),
            Some(json!({ "disabled": false })),
        )
        .await;

    // Expirado (ajustado direto no banco para não depender do relógio).
    let path = context.dir.path().join("engramind.db");
    let connection = rusqlite::Connection::open(&path).expect("open");
    connection
        .execute(
            "UPDATE tokens SET expires_at = '2000-01-01T00:00:00.000Z' WHERE id = ?1",
            [&id],
        )
        .expect("expire");
    assert_eq!(whoami(&context, &valid).await.0, StatusCode::UNAUTHORIZED);
    connection
        .execute("UPDATE tokens SET expires_at = NULL WHERE id = ?1", [&id])
        .expect("unexpire");
    assert_eq!(whoami(&context, &valid).await.0, StatusCode::OK);

    // Revogado.
    let (status, revoked) = context
        .call(
            "POST",
            &format!("/v1/tokens/{id}/revoke"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{revoked}");
    assert_eq!(revoked["status"], "revoked");
    assert_eq!(whoami(&context, &valid).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_rules_reject_escalation_invalid_scopes_and_foreign_agents() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_app("app-b").await;
    context.create_agent("app-a", "curador").await;
    let read_write = context
        .issue("app-a", None, "admin", &["read", "write"])
        .await;

    let (status, body) = context
        .call(
            "POST",
            "/v1/tokens",
            Some(&read_write),
            Some(json!({ "app": "app-a", "kind": "admin", "scopes": ["read", "write", "delete"] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "SCOPE_REQUIRED");
    assert_eq!(body["error"]["details"]["required_scope"], "delete");

    let invalid = [
        json!({ "app": "app-a", "kind": "admin", "scopes": ["write"] }),
        json!({ "app": "app-a", "kind": "admin", "scopes": [] }),
        json!({ "app": "app-a", "kind": "agent", "scopes": ["read"] }),
        json!({ "app": "app-b", "agent": "app-a/curador", "kind": "agent", "scopes": ["read"] }),
        json!({ "app": "app-a", "kind": "admin", "scopes": ["read"], "expires_at": "2000-01-01T00:00:00Z" }),
        json!({ "kind": "admin", "scopes": ["read"] }),
        json!({ "app": "app-a", "kind": "root", "scopes": ["read"] }),
    ];
    for body in invalid {
        let (status, response) = context
            .call("POST", "/v1/tokens", Some(ADMIN_TOKEN), Some(body.clone()))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {response}");
        assert_eq!(response["error"]["code"], "VALIDATION_ERROR");
    }

    let (status, body) = context
        .call(
            "POST",
            "/v1/tokens",
            Some(ADMIN_TOKEN),
            Some(json!({ "agent": "app-a/inexistente", "kind": "agent", "scopes": ["read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "AGENT_NOT_FOUND");

    // Admin com agente do mesmo app é aceito.
    let (status, body) = context
        .call(
            "POST",
            "/v1/tokens",
            Some(ADMIN_TOKEN),
            Some(json!({ "app": "app-a", "agent": "app-a/curador", "kind": "admin", "scopes": ["read"] })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // update_token também não escala.
    let id = context.issue("app-a", None, "admin", &["read"]).await;
    let id = id.split('.').nth(1).expect("id");
    let (status, body) = context
        .call(
            "PATCH",
            &format!("/v1/tokens/{id}"),
            Some(&read_write),
            Some(json!({ "scopes": ["read", "delete"] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = context
        .call(
            "PATCH",
            &format!("/v1/tokens/{id}"),
            Some(&read_write),
            Some(json!({ "scopes": ["read", "write"], "label": "novo", "expires_at": "2999-01-01T00:00:00Z" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["scopes"], json!(["read", "write"]));
    assert_eq!(body["label"], "novo");
    assert_eq!(body["expires_at"], "2999-01-01T00:00:00.000Z");
}

#[tokio::test]
async fn the_last_admin_writer_cannot_be_removed() {
    let context = TestContext::new().await;
    let bootstrap = "tk_00000001";
    let attempts = [
        ("POST", format!("/v1/tokens/{bootstrap}/revoke"), None),
        (
            "PATCH",
            format!("/v1/tokens/{bootstrap}"),
            Some(json!({ "scopes": ["read"] })),
        ),
        (
            "PATCH",
            format!("/v1/tokens/{bootstrap}"),
            Some(json!({ "expires_at": "2999-01-01T00:00:00Z" })),
        ),
        (
            "PATCH",
            "/v1/apps/bootstrap".to_owned(),
            Some(json!({ "disabled": true })),
        ),
        ("DELETE", "/v1/apps/bootstrap".to_owned(), None),
    ];
    for (method, uri, body) in &attempts {
        let (status, response) = context
            .call(method, uri, Some(ADMIN_TOKEN), body.clone())
            .await;
        if body
            .as_ref()
            .is_some_and(|body| body.get("expires_at").is_some())
        {
            // Expirar no futuro mantém o token ativo agora.
            assert_eq!(status, StatusCode::OK, "{uri}: {response}");
            continue;
        }
        assert_eq!(status, StatusCode::CONFLICT, "{method} {uri}: {response}");
        assert_eq!(response["error"]["code"], "LAST_ADMIN_TOKEN");
    }
    assert_eq!(whoami(&context, ADMIN_TOKEN).await.0, StatusCode::OK);

    context.create_app("gestor").await;
    let other = context
        .issue("gestor", None, "admin", &["read", "write", "delete"])
        .await;
    let (status, body) = context
        .call(
            "POST",
            &format!("/v1/tokens/{bootstrap}/revoke"),
            Some(&other),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = context
        .call("DELETE", "/v1/apps/gestor", Some(&other), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn slugs_are_unique_and_deletion_cascades_with_audit() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_app("app-b").await;
    let (status, body) = context
        .call(
            "POST",
            "/v1/apps",
            Some(ADMIN_TOKEN),
            Some(json!({ "slug": "app-a", "name": "X" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "SLUG_ALREADY_EXISTS");
    context.create_agent("app-a", "curador").await;
    context.create_agent("app-b", "curador").await;
    let (status, _) = context
        .call(
            "POST",
            "/v1/agents",
            Some(ADMIN_TOKEN),
            Some(json!({ "app": "app-a", "slug": "curador", "name": "X" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    for slug in ["App", "a_b", "-a", ""] {
        let (status, _) = context
            .call(
                "POST",
                "/v1/apps",
                Some(ADMIN_TOKEN),
                Some(json!({ "slug": slug, "name": "X" })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{slug}");
    }

    let secret = context
        .issue("app-a", Some("app-a/curador"), "agent", &["read"])
        .await;
    let (status, deleted) = context
        .call("DELETE", "/v1/apps/app-a", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{deleted}");
    assert_eq!(deleted["slug"], "app-a");
    assert_eq!(whoami(&context, &secret).await.0, StatusCode::UNAUTHORIZED);
    let (status, body) = context
        .call("GET", "/v1/agents/app-a%2Fcurador", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "AGENT_NOT_FOUND");
    let (status, _) = context
        .call("GET", "/v1/agents/app-b%2Fcurador", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, audit) = context
        .call(
            "GET",
            "/v1/audit?entity=app&action=app.delete",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let event = &audit["events"][0];
    assert_eq!(event["before"]["slug"], "app-a");
    assert_eq!(event["token_id"], "tk_00000001");
    assert!(event["after"].is_null());
}

#[tokio::test]
async fn audit_never_contains_secrets_or_digests() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    let secret = context.issue("app-a", None, "admin", &["read"]).await;
    let id = secret.split('.').nth(1).expect("id");
    context
        .call(
            "PATCH",
            &format!("/v1/tokens/{id}"),
            Some(ADMIN_TOKEN),
            Some(json!({ "label": "x" })),
        )
        .await;
    context
        .call(
            "POST",
            &format!("/v1/tokens/{id}/revoke"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    let (status, audit) = context
        .call("GET", "/v1/audit?limit=200", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let text = audit.to_string();
    let actions: Vec<&str> = audit["events"]
        .as_array()
        .expect("events")
        .iter()
        .filter_map(|event| event["action"].as_str())
        .collect();
    assert_eq!(
        actions,
        [
            "token.revoke",
            "token.update",
            "token.issue",
            "app.create",
            "token.bootstrap",
            "app.create"
        ]
    );
    let raw = secret.rsplit('.').next().expect("segredo");
    let bootstrap_secret = ADMIN_TOKEN.rsplit('.').next().expect("segredo");
    for forbidden in [raw, bootstrap_secret, "secret_hash", "digest"] {
        assert!(!text.contains(forbidden), "{forbidden} na auditoria");
    }
}

#[tokio::test]
async fn idempotency_replays_the_same_response_and_rejects_another_body() {
    let context = TestContext::new().await;
    let headers = [("Idempotency-Key", "criar-app-a")];
    let body = json!({ "slug": "app-a", "name": "App A" });
    let (first_status, _, first) = context
        .call_with(
            "POST",
            "/v1/apps",
            Some(ADMIN_TOKEN),
            Some(body.clone()),
            &headers,
        )
        .await;
    assert_eq!(first_status, StatusCode::CREATED);
    // Mesmo corpo com outra ordem de chaves: repetição.
    let reordered = json!({ "name": "App A", "slug": "app-a" });
    let (status, _, again) = context
        .call_with(
            "POST",
            "/v1/apps",
            Some(ADMIN_TOKEN),
            Some(reordered),
            &headers,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(again, first);

    let (status, _, conflict) = context
        .call_with(
            "POST",
            "/v1/apps",
            Some(ADMIN_TOKEN),
            Some(json!({ "slug": "app-b", "name": "B" })),
            &headers,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "IDEMPOTENCY_KEY_CONFLICT");
    let (status, _, conflict) = context
        .call_with(
            "POST",
            "/v1/agents",
            Some(ADMIN_TOKEN),
            Some(json!({ "app": "app-a", "slug": "x", "name": "x" })),
            &headers,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "outra operacao: {conflict}");

    // Outro token pode usar a mesma chave.
    let other = context
        .issue("app-a", None, "admin", &["read", "write"])
        .await;
    let (status, _, _) = context
        .call_with(
            "POST",
            "/v1/apps",
            Some(&other),
            Some(json!({ "slug": "app-c", "name": "C" })),
            &headers,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // issue_token repetido devolve o token sem o segredo.
    let issue = json!({ "app": "app-a", "kind": "admin", "scopes": ["read"] });
    let key = [("Idempotency-Key", "emitir-1")];
    let (_, _, issued) = context
        .call_with(
            "POST",
            "/v1/tokens",
            Some(ADMIN_TOKEN),
            Some(issue.clone()),
            &key,
        )
        .await;
    assert!(issued["secret"].is_string());
    let (status, _, replay) = context
        .call_with("POST", "/v1/tokens", Some(ADMIN_TOKEN), Some(issue), &key)
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(replay["token"]["id"], issued["token"]["id"]);
    assert!(replay.get("secret").is_none(), "{replay}");

    // Exclusão repetida devolve a resposta gravada, não 404.
    let key = [("Idempotency-Key", "excluir-c")];
    let (status, _, deleted) = context
        .call_with("DELETE", "/v1/apps/app-c", Some(ADMIN_TOKEN), None, &key)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, again) = context
        .call_with("DELETE", "/v1/apps/app-c", Some(ADMIN_TOKEN), None, &key)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, deleted);

    let (status, _, invalid) = context
        .call_with(
            "POST",
            "/v1/apps",
            Some(ADMIN_TOKEN),
            Some(json!({ "slug": "z", "name": "z" })),
            &[("Idempotency-Key", "com espaco")],
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid}");
}

#[tokio::test]
async fn lists_are_paginated_by_cursor() {
    let context = TestContext::new().await;
    for slug in ["a1", "a2", "a3"] {
        context.create_app(slug).await;
    }
    let (status, first) = context
        .call("GET", "/v1/apps?limit=2", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let slugs = |page: &Value| -> Vec<String> {
        page["apps"]
            .as_array()
            .expect("apps")
            .iter()
            .map(|app| app["slug"].as_str().expect("slug").to_owned())
            .collect()
    };
    assert_eq!(slugs(&first), ["bootstrap", "a1"]);
    let cursor = first["next_cursor"].as_str().expect("cursor");
    let (_, second) = context
        .call(
            "GET",
            &format!("/v1/apps?limit=2&cursor={cursor}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(slugs(&second), ["a2", "a3"]);
    assert!(second["next_cursor"].is_null());

    for query in ["limit=0", "limit=201", "cursor=%25%25"] {
        let (status, body) = context
            .call("GET", &format!("/v1/apps?{query}"), Some(ADMIN_TOKEN), None)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
    }

    let (_, audit) = context
        .call("GET", "/v1/audit?limit=1", Some(ADMIN_TOKEN), None)
        .await;
    let cursor = audit["next_cursor"].as_str().expect("cursor");
    let (_, next) = context
        .call(
            "GET",
            &format!("/v1/audit?limit=1&cursor={cursor}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert!(next["events"][0]["id"].as_i64() < audit["events"][0]["id"].as_i64());
}

#[tokio::test]
async fn rate_limit_answers_429_with_retry_after() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    let token = context.issue("app-a", None, "admin", &["read"]).await;
    let (status, body) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "limits.rate_per_minute": 3 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for _ in 0..3 {
        assert_eq!(whoami(&context, &token).await.0, StatusCode::OK);
    }
    let (status, headers, body) = context
        .call_with("GET", "/v1/whoami", Some(&token), None, &[])
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "RATE_LIMITED");
    let retry: u64 = headers[header::RETRY_AFTER]
        .to_str()
        .expect("retry")
        .parse()
        .expect("seconds");
    assert!((1..=60).contains(&retry), "{retry}");
}

#[tokio::test]
async fn last_used_at_is_written_in_batch() {
    let context = TestContext::new().await;
    let (_, before) = context
        .call("GET", "/v1/tokens", Some(ADMIN_TOKEN), None)
        .await;
    assert!(before["tokens"][0]["last_used_at"].is_null());
    context.core.flush_usage().await.expect("flush");
    let (_, after) = context
        .call("GET", "/v1/tokens", Some(ADMIN_TOKEN), None)
        .await;
    assert!(after["tokens"][0]["last_used_at"].is_string(), "{after}");
}

#[tokio::test]
async fn settings_validate_keys_and_values_and_reset_with_null() {
    let context = TestContext::new().await;
    let (status, all) = context
        .call("GET", "/v1/settings", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let rate = &all["settings"]
        .as_array()
        .expect("settings")
        .iter()
        .find(|setting| setting["key"] == "limits.rate_per_minute")
        .expect("rate")
        .clone();
    assert_eq!(rate["value"], 600);
    assert_eq!(rate["default"], 600);
    assert_eq!(rate["type"], "integer");
    assert!(rate["updated_at"].is_null());

    for (body, code) in [
        (json!({ "limits.inexistente": 1 }), "SETTING_UNKNOWN"),
        (json!({ "limits.rate_per_minute": 0 }), "SETTING_INVALID"),
        (json!({ "limits.max_batch": "100" }), "SETTING_INVALID"),
        (json!({}), "VALIDATION_ERROR"),
        (
            json!({ "limits.max_batch": 5, "limits.rate_per_minute": -1 }),
            "SETTING_INVALID",
        ),
    ] {
        let (status, response) = context
            .call("PUT", "/v1/settings", Some(ADMIN_TOKEN), Some(body.clone()))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(response["error"]["code"], code, "{body}");
    }
    // Tudo ou nada: o max_batch válido do último pedido não foi gravado.
    let (_, one) = context
        .call(
            "GET",
            "/v1/settings?key=limits.max_batch",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(one["settings"][0]["value"], 100);

    let (status, updated) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "limits.max_batch": 5 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let batch = updated["settings"]
        .as_array()
        .expect("settings")
        .iter()
        .find(|setting| setting["key"] == "limits.max_batch")
        .expect("batch")
        .clone();
    assert_eq!(batch["value"], 5);
    assert!(batch["updated_by_app"].is_string());
    assert_eq!(context.core.settings_snapshot().max_batch(), 5);

    let (_, reset) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "limits.max_batch": null })),
        )
        .await;
    assert!(reset.to_string().contains("\"limits.max_batch\""));
    assert_eq!(context.core.settings_snapshot().max_batch(), 100);

    let (status, body) = context
        .call("GET", "/v1/settings?key=x.y", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "SETTING_UNKNOWN");
}

#[tokio::test]
async fn status_reports_version_database_and_counts() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    let (status, body) = context
        .call("GET", "/v1/status", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["database"]["status"], "ok");
    assert_eq!(body["backup"]["status"], "skip");
    assert_eq!(body["counts"]["apps"], 2);
    assert_eq!(body["counts"]["active_tokens"], 1);
}
