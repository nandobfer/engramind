use std::time::Duration;

use axum::{Router, body::Body, http::Request, response::Response};
use engramind::{HttpSettings, build_app, store::SqliteStore};
use http_body_util::BodyExt as _;
use tempfile::TempDir;
use tower::ServiceExt as _;

pub const MAX_BODY_BYTES: usize = 1024;

/// SQLite próprio em `tempdir`, migrado, e o router sobre ele.
pub struct TestContext {
    pub dir: TempDir,
    pub store: SqliteStore,
    pub app: Router,
}

impl TestContext {
    pub async fn new() -> Self {
        let context = Self::unmigrated();
        context.store.migrate().await.expect("migrate");
        context
    }

    pub fn unmigrated() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("engramind.db"), 2).expect("open store");
        let app = build_app(
            store.clone(),
            HttpSettings {
                request_timeout: Duration::from_secs(5),
                max_body_bytes: MAX_BODY_BYTES,
            },
        );
        Self { dir, store, app }
    }

    pub async fn send(&self, request: Request<Body>) -> Response {
        self.app.clone().oneshot(request).await.expect("response")
    }

    pub async fn get(&self, uri: &str) -> Response {
        self.send(Request::get(uri).body(Body::empty()).expect("request"))
            .await
    }
}

pub async fn json(response: Response) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("JSON body")
}
