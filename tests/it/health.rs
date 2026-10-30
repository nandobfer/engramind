use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};

use crate::support::{MAX_BODY_BYTES, TestContext, json};

#[tokio::test]
async fn live_and_ready_respond_when_the_database_is_migrated() {
    let context = TestContext::new().await;

    let live = context.get("/health/live").await;
    assert_eq!(live.status(), StatusCode::OK);
    assert_eq!(json(live).await, serde_json::json!({ "status": "ok" }));

    let ready = context.get("/health/ready").await;
    assert_eq!(ready.status(), StatusCode::OK);
    let body = json(ready).await;
    assert_eq!(body["status"], "ready");
    assert_eq!(body["database"], "ok");
    assert_eq!(
        body["migrations"]["applied"],
        body["migrations"]["expected"]
    );
}

#[tokio::test]
async fn ready_reports_pending_migrations_in_the_envelope() {
    let context = TestContext::unmigrated();
    let response = context.get("/health/ready").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = json(response).await;
    assert_eq!(body["error"]["code"], "MIGRATIONS_PENDING");
    assert_eq!(body["error"]["details"]["applied"], 0);

    context.store.migrate().await.expect("migrate");
    assert_eq!(context.get("/health/ready").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn root_and_docs_redirect_relatively() {
    let context = TestContext::new().await;
    for uri in ["/", "/docs"] {
        let response = context.get(uri).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(response.headers()[header::LOCATION], "docs/", "{uri}");
    }
    let swagger = context.get("/docs/").await;
    assert_eq!(swagger.status(), StatusCode::OK);
    assert_eq!(swagger.headers()[header::CACHE_CONTROL], "no-store");

    let config = context.get("/docs/swagger-initializer.js").await;
    assert_eq!(config.status(), StatusCode::OK);
    let script = String::from_utf8(
        http_body_util::BodyExt::collect(config.into_body())
            .await
            .expect("body")
            .to_bytes()
            .to_vec(),
    )
    .expect("utf8");
    assert!(script.contains("../openapi.json"), "{script}");
}

#[tokio::test]
async fn unknown_routes_and_methods_use_the_envelope() {
    let context = TestContext::new().await;

    let missing = context.get("/v1/nada").await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(missing).await["error"]["code"], "ROUTE_NOT_FOUND");

    let wrong_method = context
        .send(
            Request::delete("/health/live")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
    assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        json(wrong_method).await["error"]["code"],
        "METHOD_NOT_ALLOWED"
    );
}

#[tokio::test]
async fn oversized_bodies_are_rejected_with_413() {
    let context = TestContext::new().await;
    let response = context
        .send(
            Request::post("/health/live")
                .header(header::CONTENT_LENGTH, MAX_BODY_BYTES + 1)
                .body(Body::from(vec![b'x'; MAX_BODY_BYTES + 1]))
                .expect("request"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(response).await["error"]["code"], "PAYLOAD_TOO_LARGE");
}

#[tokio::test]
async fn request_id_is_preserved_when_valid_and_generated_otherwise() {
    let context = TestContext::new().await;
    let id = "1b4e28ba-2fa1-41d2-883f-0016d3cca427";
    let preserved = context
        .send(
            Request::get("/health/live")
                .header("x-request-id", id)
                .body(Body::empty())
                .expect("request"),
        )
        .await;
    assert_eq!(preserved.headers()["x-request-id"], id);

    for request in [
        Request::get("/health/live")
            .header("x-request-id", "nao-e-uuid")
            .body(Body::empty())
            .expect("request"),
        Request::get("/v1/nada")
            .body(Body::empty())
            .expect("request"),
    ] {
        let response = context.send(request).await;
        let generated = response.headers()["x-request-id"].to_str().expect("header");
        assert!(uuid::Uuid::parse_str(generated).is_ok(), "{generated}");
    }
}

#[tokio::test]
async fn cors_stays_closed() {
    let context = TestContext::new().await;
    let preflight = context
        .send(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/health/live")
                .header(header::ORIGIN, "https://evil.example")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
    let simple = context
        .send(
            Request::get("/health/live")
                .header(header::ORIGIN, "https://evil.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
    for response in [preflight, simple] {
        assert!(
            response
                .headers()
                .keys()
                .all(|name| !name.as_str().starts_with("access-control-")),
            "{:?}",
            response.headers()
        );
    }
}
