pub mod llm;
pub mod webhook;

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
    response::Response,
};
use engramind::{
    HttpSettings, build_app,
    config::{AllowedHosts, DatabaseSettings, ModelsSettings, Secret, SecretKey},
    core::Core,
    mcp::McpSettings,
    providers::embeddings::HashEmbedder,
    store::SqliteStore,
};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt as _;

pub const MAX_BODY_BYTES: usize = 64 * 1024;
pub const PEPPER: &str = "pepper-de-teste-com-mais-de-32-caracteres";
/// Token do bootstrap (admin `read+write+delete`, app `bootstrap`). Só existe nos testes.
pub const ADMIN_TOKEN: &str = "eng.tk_00000001.segredo-de-teste-do-bootstrap-com-43-caracteres";
pub const MCP_HOST: &str = "localhost";
pub const MCP_ACCEPT: &str = "application/json, text/event-stream";

/// SQLite próprio em `tempdir`, migrado, com o token do bootstrap, e o router sobre ele.
pub struct TestContext {
    pub dir: TempDir,
    pub store: SqliteStore,
    pub core: Core<SqliteStore>,
    pub app: Router,
}

impl TestContext {
    pub async fn new() -> Self {
        let context = Self::unmigrated();
        context.store.migrate().await.expect("migrate");
        context.core.reload_settings().await.expect("settings");
        context.core.sync_embeddings().await.expect("embeddings");
        context
            .core
            .bootstrap(Some(&Secret::new(ADMIN_TOKEN.to_owned())))
            .await
            .expect("bootstrap");
        context
    }

    pub fn unmigrated() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("engramind.db");
        let store = SqliteStore::open(&path, 2).expect("open store");
        let core = Core::new(
            store.clone(),
            Secret::new(PEPPER.to_owned()),
            SecretKey::new([7; 32]),
            DatabaseSettings {
                path,
                read_pool_size: 2,
                litestream_enabled: false,
            },
            ModelsSettings {
                path: dir.path().join("models"),
                onnx_runtime: None,
            },
        );
        // O provedor local padrão vira embeddings por hash: sem ONNX Runtime nem download.
        core.set_local_embedder(Arc::new(HashEmbedder::new()));
        let app = build_app(
            core.clone(),
            HttpSettings {
                request_timeout: Duration::from_secs(5),
                max_body_bytes: MAX_BODY_BYTES,
            },
            &McpSettings {
                allowed_hosts: AllowedHosts::default(),
            },
        );
        Self {
            dir,
            store,
            core,
            app,
        }
    }

    /// Outro `Core` (e router) sobre o mesmo banco, como depois de reiniciar o serviço: sem os
    /// caches em memória do primeiro.
    pub async fn reopened(&self) -> Router {
        let core = Core::new(
            self.store.clone(),
            Secret::new(PEPPER.to_owned()),
            SecretKey::new([7; 32]),
            DatabaseSettings {
                path: self.dir.path().join("engramind.db"),
                read_pool_size: 2,
                litestream_enabled: false,
            },
            ModelsSettings {
                path: self.dir.path().join("models"),
                onnx_runtime: None,
            },
        );
        core.set_local_embedder(Arc::new(HashEmbedder::new()));
        core.reload_settings().await.expect("settings");
        build_app(
            core,
            HttpSettings {
                request_timeout: Duration::from_secs(5),
                max_body_bytes: MAX_BODY_BYTES,
            },
            &McpSettings {
                allowed_hosts: AllowedHosts::default(),
            },
        )
    }

    pub async fn send(&self, request: Request<Body>) -> Response {
        self.app.clone().oneshot(request).await.expect("response")
    }

    pub async fn get(&self, uri: &str) -> Response {
        self.send(Request::get(uri).body(Body::empty()).expect("request"))
            .await
    }

    /// Chamada REST com token opcional e corpo JSON opcional.
    pub async fn call(
        &self,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let (status, _, body) = self.call_with(method, uri, token, body, &[]).await;
        (status, body)
    }

    pub async fn call_with(
        &self,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let body = match body {
            Some(body) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = self.send(builder.body(body).expect("request")).await;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        };
        (status, headers, value)
    }

    /// Cria um app pelo REST com o token do bootstrap.
    pub async fn create_app(&self, slug: &str) -> Value {
        let (status, body) = self
            .call(
                "POST",
                "/v1/apps",
                Some(ADMIN_TOKEN),
                Some(json!({ "slug": slug, "name": slug })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    pub async fn create_agent(&self, app: &str, slug: &str) -> Value {
        let (status, body) = self
            .call(
                "POST",
                "/v1/agents",
                Some(ADMIN_TOKEN),
                Some(json!({ "app": app, "slug": slug, "name": slug })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    /// Cria um canal pelo REST com o token do bootstrap.
    pub async fn create_channel(&self, slug: &str) -> Value {
        let (status, body) = self
            .call(
                "POST",
                "/v1/channels",
                Some(ADMIN_TOKEN),
                Some(json!({ "slug": slug, "name": slug })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    /// Assina um agente a um alvo pelo REST com o token do bootstrap.
    pub async fn subscribe(
        &self,
        agent: &str,
        target: &str,
        mode: &str,
        access: Option<&[&str]>,
    ) -> Value {
        let (status, body) = self
            .call(
                "POST",
                "/v1/subscriptions",
                Some(ADMIN_TOKEN),
                Some(json!({ "agent": agent, "target": target, "mode": mode, "access": access })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    /// Emite um token e devolve o segredo completo.
    pub async fn issue(
        &self,
        app: &str,
        agent: Option<&str>,
        kind: &str,
        scopes: &[&str],
    ) -> String {
        let (status, body) = self
            .call(
                "POST",
                "/v1/tokens",
                Some(ADMIN_TOKEN),
                Some(json!({ "app": app, "agent": agent, "kind": kind, "scopes": scopes })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["secret"].as_str().expect("secret").to_owned()
    }

    /// Conexão direta com o banco do teste, para conferir invariantes.
    pub fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.dir.path().join("engramind.db")).expect("open db")
    }

    /// Contagem de uma consulta `SELECT COUNT(*) …`.
    pub fn count(&self, sql: &str) -> i64 {
        self.db()
            .query_row(sql, [], |row| row.get(0))
            .expect("count")
    }

    /// Grava uma memória pelo REST e devolve a resposta (`201` ou `200` em duplicata).
    pub async fn create_memory(&self, token: &str, channel: &str, text: &str) -> Value {
        let (status, body) = self
            .call(
                "POST",
                "/v1/memories",
                Some(token),
                Some(json!({ "channel": channel, "text": text })),
            )
            .await;
        assert!(status.is_success(), "{status} {body}");
        body
    }

    /// Requisição JSON-RPC ao `/mcp`.
    pub async fn rpc(&self, token: &str, method: &str, params: Value) -> (StatusCode, Value) {
        self.rpc_with(token, method, params, &[]).await
    }

    /// Como `rpc`, com cabeçalhos extras (por exemplo `Engramind-Channels`).
    pub async fn rpc_with(
        &self,
        token: &str,
        method: &str,
        params: Value,
        headers: &[(&str, &str)],
    ) -> (StatusCode, Value) {
        let mut builder = Request::post("/mcp")
            .header(header::HOST, MCP_HOST)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, MCP_ACCEPT)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = builder
            .body(Body::from(
                json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params })
                    .to_string(),
            ))
            .expect("MCP request");
        let response = self.send(request).await;
        let status = response.status();
        (status, json(response).await)
    }

    /// `tools/call`; devolve o `result` (com `isError` e `structuredContent`).
    pub async fn call_tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let (status, body) = self
            .rpc(
                token,
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// Sobe o router numa porta efêmera (para a CLI) e devolve a URL base.
    pub async fn serve(&self) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("addr");
        let app = self.app.clone();
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        format!("http://{address}")
    }
}

pub async fn json(response: Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("JSON body")
}
