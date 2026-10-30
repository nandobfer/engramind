//! MCP em `POST /mcp`: transporte, filtro de ferramentas, hints e erros de domínio.

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use engramind::core::operations::{OPERATIONS, RequiredKind};
use serde_json::{Value, json};

use crate::support::{ADMIN_TOKEN, MCP_ACCEPT, MCP_HOST, TestContext, json};

fn tool_names(response: &Value) -> Vec<String> {
    response["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools: {response}"))
        .iter()
        .map(|tool| tool["name"].as_str().expect("name").to_owned())
        .collect()
}

async fn agent_token(context: &TestContext, scopes: &[&str]) -> String {
    context.create_app("app-a").await;
    context.create_agent("app-a", "curador").await;
    context
        .issue("app-a", Some("app-a/curador"), "agent", scopes)
        .await
}

#[tokio::test]
async fn initialize_and_tools_list_follow_the_token() {
    let context = TestContext::new().await;
    let (status, init) = context
        .rpc(
            ADMIN_TOKEN,
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "engramind");
    assert_eq!(
        init["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );

    let (_, admin) = context.rpc(ADMIN_TOKEN, "tools/list", json!({})).await;
    let names = tool_names(&admin);
    let expected: Vec<&str> = OPERATIONS.iter().map(|operation| operation.name).collect();
    assert_eq!(names, expected);

    let agent = agent_token(&context, &["read", "write", "delete"]).await;
    let (_, listed) = context.rpc(&agent, "tools/list", json!({})).await;
    let names = tool_names(&listed);
    let expected: Vec<&str> = OPERATIONS
        .iter()
        .filter(|operation| operation.kind == RequiredKind::Any)
        .map(|operation| operation.name)
        .collect();
    assert_eq!(names, expected);

    let read_admin = context.issue("app-a", None, "admin", &["read"]).await;
    let (_, listed) = context.rpc(&read_admin, "tools/list", json!({})).await;
    let names = tool_names(&listed);
    assert!(names.contains(&"list_tokens".to_owned()));
    assert!(!names.contains(&"issue_token".to_owned()));
    assert!(!names.contains(&"revoke_token".to_owned()));
}

#[tokio::test]
async fn tools_carry_the_catalog_hints_and_flat_arguments() {
    let context = TestContext::new().await;
    let (_, listed) = context.rpc(ADMIN_TOKEN, "tools/list", json!({})).await;
    for tool in listed["result"]["tools"].as_array().expect("tools") {
        let name = tool["name"].as_str().expect("name");
        let operation = OPERATIONS
            .iter()
            .find(|operation| operation.name == name)
            .expect("operation");
        let annotations = &tool["annotations"];
        assert_eq!(annotations["readOnlyHint"], operation.read_only, "{name}");
        assert_eq!(
            annotations["destructiveHint"], operation.destructive,
            "{name}"
        );
        assert_eq!(
            annotations["idempotentHint"], operation.idempotent,
            "{name}"
        );
        assert_eq!(tool["description"], operation.summary, "{name}");
        assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
    }
    let update = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|tool| tool["name"] == "update_app")
        .expect("update_app");
    let properties = update["inputSchema"]["properties"]
        .as_object()
        .expect("properties");
    for field in ["app", "name", "description", "disabled", "idempotency_key"] {
        assert!(properties.contains_key(field), "{field}: {update}");
    }
}

#[tokio::test]
async fn hidden_tools_and_domain_errors_return_the_rest_envelope() {
    let context = TestContext::new().await;
    let agent = agent_token(&context, &["read"]).await;
    let hidden = context
        .call_tool(&agent, "create_app", json!({ "slug": "x", "name": "x" }))
        .await;
    assert_eq!(hidden["result"]["isError"], true, "{hidden}");
    let error = &hidden["result"]["structuredContent"]["error"];
    assert_eq!(error["code"], "SCOPE_REQUIRED");
    assert_eq!(error["details"]["required_kind"], "admin");

    let missing = context
        .call_tool(ADMIN_TOKEN, "get_app", json!({ "app": "nao-existe" }))
        .await;
    assert_eq!(missing["result"]["isError"], true);
    assert_eq!(
        missing["result"]["structuredContent"]["error"]["code"],
        "APP_NOT_FOUND"
    );

    let invalid = context
        .call_tool(ADMIN_TOKEN, "get_app", json!({ "nada": 1 }))
        .await;
    assert_eq!(invalid["result"]["isError"], true, "{invalid}");
    assert_eq!(
        invalid["result"]["structuredContent"]["error"]["code"],
        "VALIDATION_ERROR"
    );

    let created = context
        .call_tool(
            ADMIN_TOKEN,
            "create_app",
            json!({ "slug": "app-mcp", "name": "MCP", "idempotency_key": "mcp-1" }),
        )
        .await;
    assert_eq!(created["result"]["isError"], false, "{created}");
    let app = &created["result"]["structuredContent"];
    assert_eq!(app["slug"], "app-mcp");
    let replay = context
        .call_tool(
            ADMIN_TOKEN,
            "create_app",
            json!({ "slug": "app-mcp", "name": "MCP", "idempotency_key": "mcp-1" }),
        )
        .await;
    assert_eq!(replay["result"]["structuredContent"], *app);

    let updated = context
        .call_tool(
            ADMIN_TOKEN,
            "update_settings",
            json!({ "settings": { "limits.max_batch": 7 } }),
        )
        .await;
    assert_eq!(updated["result"]["isError"], false, "{updated}");

    let guide = context
        .call_tool(
            &agent,
            "get_guide",
            json!({ "guide": "tokens-e-escopos", "section": "escopos" }),
        )
        .await;
    let text = guide["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(text.starts_with("## Escopos"), "{text}");
    let hidden_guide = context
        .call_tool(&agent, "get_guide", json!({ "guide": "configuracoes" }))
        .await;
    assert_eq!(
        hidden_guide["result"]["structuredContent"]["error"]["code"],
        "GUIDE_NOT_FOUND"
    );
}

fn raw(
    host: &str,
    accept: &str,
    content_type: &str,
    token: Option<&str>,
    body: &str,
) -> Request<Body> {
    let mut builder = Request::post("/mcp")
        .header(header::HOST, host)
        .header(header::ACCEPT, accept)
        .header(header::CONTENT_TYPE, content_type);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_owned())).expect("request")
}

#[tokio::test]
async fn transport_failures_use_the_standard_envelope() {
    let context = TestContext::new().await;
    let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } } })
    .to_string();

    let cases = [
        (
            raw(MCP_HOST, MCP_ACCEPT, "application/json", None, &init),
            StatusCode::UNAUTHORIZED,
            "AUTH_INVALID",
        ),
        (
            raw(
                "attacker.example",
                MCP_ACCEPT,
                "application/json",
                Some(ADMIN_TOKEN),
                &init,
            ),
            StatusCode::FORBIDDEN,
            "MCP_HOST_FORBIDDEN",
        ),
        (
            raw(
                MCP_HOST,
                "text/html",
                "application/json",
                Some(ADMIN_TOKEN),
                &init,
            ),
            StatusCode::NOT_ACCEPTABLE,
            "MCP_NOT_ACCEPTABLE",
        ),
        (
            raw(MCP_HOST, MCP_ACCEPT, "text/plain", Some(ADMIN_TOKEN), &init),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "MCP_UNSUPPORTED_MEDIA_TYPE",
        ),
    ];
    for (request, status, code) in cases {
        let response = context.send(request).await;
        assert_eq!(response.status(), status, "{code}");
        assert_eq!(json(response).await["error"]["code"], code);
    }

    let response = context
        .send(
            Request::get("/mcp")
                .header(header::HOST, MCP_HOST)
                .header(header::ACCEPT, MCP_ACCEPT)
                .header(header::AUTHORIZATION, format!("Bearer {ADMIN_TOKEN}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "POST");
    assert_eq!(json(response).await["error"]["code"], "METHOD_NOT_ALLOWED");
}
