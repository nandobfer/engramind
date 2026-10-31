//! Logs de requisição e do MCP: identidade e operação no span, nunca tokens, segredos ou
//! argumentos. Cada teste roda num processo próprio (nextest), então o subscriber com escopo
//! não disputa o cache de interesse do `tracing` com outros testes.

use serde_json::{Value, json};

use crate::support::{ADMIN_TOKEN, TestContext};

#[derive(Clone, Default)]
struct LogBuffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log lock").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn events(buffer: &LogBuffer) -> (String, Vec<Value>) {
    let output = String::from_utf8(buffer.0.lock().expect("log lock").clone()).expect("UTF-8");
    let events = output
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON log line"))
        .collect();
    (output, events)
}

#[tokio::test]
async fn requests_and_tool_calls_log_identity_without_secrets_or_arguments() {
    let context = TestContext::new().await;
    let buffer = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter(engramind::observability::log_filter("debug").expect("filter"))
        .with_writer(buffer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::callsite::rebuild_interest_cache();

    let (status, issued) = context
        .call(
            "POST",
            "/v1/apps",
            Some(ADMIN_TOKEN),
            Some(json!({ "slug": "app-a", "name": "nome-sensivel-xyz" })),
        )
        .await;
    assert!(status.is_success(), "{issued}");
    let secret = context.issue("app-a", None, "admin", &["read"]).await;
    context
        .call_tool(
            ADMIN_TOKEN,
            "search_docs",
            json!({ "q": "consulta-secreta-xyz" }),
        )
        .await;
    context
        .call(
            "GET",
            "/v1/whoami",
            Some("eng.tk_ffffffff.invalido-mas-com-32-caracteres-x"),
            None,
        )
        .await;

    let (output, events) = events(&buffer);
    let request = events
        .iter()
        .find(|event| {
            event["fields"]["event"] == "http.request.completed"
                && event["span"]["route"] == "/v1/apps"
        })
        .unwrap_or_else(|| panic!("evento de requisicao: {output}"));
    assert_eq!(request["level"], "INFO");
    assert_eq!(request["span"]["app"], "bootstrap");
    assert_eq!(request["span"]["token_id"], "tk_00000001");
    assert_eq!(request["span"]["operation"], "create_app");

    let tool = events
        .iter()
        .find(|event| event["fields"]["event"] == "mcp.tool.completed")
        .unwrap_or_else(|| panic!("evento MCP: {output}"));
    assert_eq!(tool["level"], "DEBUG");
    assert_eq!(tool["fields"]["tool"], "search_docs");
    assert_eq!(tool["fields"]["outcome"], "success");

    let mcp = events
        .iter()
        .find(|event| {
            event["fields"]["event"] == "http.request.completed" && event["span"]["route"] == "/mcp"
        })
        .expect("requisicao MCP");
    assert_eq!(mcp["level"], "DEBUG");

    for forbidden in [
        ADMIN_TOKEN,
        ADMIN_TOKEN.rsplit('.').next().expect("segredo"),
        secret.as_str(),
        secret.rsplit('.').next().expect("segredo"),
        "invalido-mas-com-32-caracteres-x",
        "consulta-secreta-xyz",
        "nome-sensivel-xyz",
        "Bearer",
    ] {
        assert!(!output.contains(forbidden), "{forbidden} no log");
    }
}

#[tokio::test]
async fn memory_text_never_reaches_the_logs() {
    let context = TestContext::new().await;
    let buffer = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter(engramind::observability::log_filter("debug").expect("filter"))
        .with_writer(buffer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::callsite::rebuild_interest_cache();

    context.create_channel("eng/global").await;
    let created = context
        .create_memory(ADMIN_TOKEN, "eng/global", "texto-sigiloso-da-memoria abc")
        .await;
    let id = created["memory"]["id"].as_str().expect("id");
    context
        .call(
            "PATCH",
            &format!("/v1/memories/{id}"),
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "edicao-sigilosa-da-memoria abc" })),
        )
        .await;
    context
        .call(
            "POST",
            "/v1/memories/search",
            Some(ADMIN_TOKEN),
            Some(json!({ "query": "consulta-sigilosa" })),
        )
        .await;
    context
        .call_tool(
            ADMIN_TOKEN,
            "create_memory",
            json!({ "channel": "eng/global", "text": "texto-sigiloso-pelo-mcp" }),
        )
        .await;

    let (output, events) = events(&buffer);
    assert!(
        events
            .iter()
            .any(|event| event["span"]["operation"] == "create_memory"),
        "{output}"
    );
    for forbidden in [
        "texto-sigiloso-da-memoria",
        "edicao-sigilosa-da-memoria",
        "consulta-sigilosa",
        "texto-sigiloso-pelo-mcp",
    ] {
        assert!(!output.contains(forbidden), "{forbidden} no log");
    }
}

#[tokio::test]
async fn context_polls_with_304_are_debug_and_never_log_the_block() {
    let context = TestContext::new().await;
    let buffer = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter(engramind::observability::log_filter("debug").expect("filter"))
        .with_writer(buffer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::callsite::rebuild_interest_cache();

    context.create_channel("eng/global").await;
    context
        .create_memory(ADMIN_TOKEN, "eng/global", "texto-sigiloso-do-contexto abc")
        .await;
    let (_, body) = context
        .call("GET", "/v1/context", Some(ADMIN_TOKEN), None)
        .await;
    let version = body["version"].as_str().expect("version").to_owned();
    let (status, _, _) = context
        .call_with(
            "GET",
            "/v1/context",
            Some(ADMIN_TOKEN),
            None,
            &[("If-None-Match", &version)],
        )
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_MODIFIED);
    context
        .rpc(
            ADMIN_TOKEN,
            "resources/read",
            json!({ "uri": "engramind://context" }),
        )
        .await;

    let (output, events) = events(&buffer);
    let completed: Vec<&Value> = events
        .iter()
        .filter(|event| {
            event["fields"]["event"] == "http.request.completed"
                && event["span"]["route"] == "/v1/context"
        })
        .collect();
    assert_eq!(completed.len(), 2, "{output}");
    assert_eq!(completed[0]["level"], "INFO");
    assert_eq!(completed[0]["fields"]["status"], 200);
    assert_eq!(completed[1]["level"], "DEBUG");
    assert_eq!(completed[1]["fields"]["status"], 304);
    assert!(
        events
            .iter()
            .any(|event| event["fields"]["event"] == "mcp.resource.completed"),
        "{output}"
    );
    assert!(
        !output.contains("texto-sigiloso-do-contexto"),
        "texto no log"
    );
}
