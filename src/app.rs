use std::time::Duration;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, Method, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{MethodRouter, get},
};
use utoipa_swagger_ui::{Config as SwaggerConfig, SwaggerUi};

use crate::{config::Settings, error::ApiError, health, observability, openapi, store::AppStore};

#[derive(Clone)]
pub struct AppState {
    pub store: AppStore,
}

/// Limites HTTP derivados de `Settings`.
#[derive(Clone, Copy, Debug)]
pub struct HttpSettings {
    pub request_timeout: Duration,
    pub max_body_bytes: usize,
}

impl From<&Settings> for HttpSettings {
    fn from(settings: &Settings) -> Self {
        Self {
            request_timeout: settings.request_timeout,
            max_body_bytes: settings.max_body_bytes,
        }
    }
}

/// Rotas servidas pela Swagger UI (fora de `routes()`), também documentadas no OpenAPI.
pub const SWAGGER_ROUTES: &[&str] = &["/docs", "/openapi.json"];

/// Ponto único de registro das rotas: o teste de contrato compara esta lista com o OpenAPI.
fn routes() -> Vec<(&'static str, MethodRouter<AppState>)> {
    vec![
        ("/", get(openapi::root_docs_redirect)),
        ("/health/live", get(health::live)),
        ("/health/ready", get(health::ready)),
    ]
}

/// Caminhos de todas as rotas do router (exceto os ativos internos da Swagger UI).
pub fn route_paths() -> Vec<&'static str> {
    routes()
        .into_iter()
        .map(|(path, _)| path)
        .chain(SWAGGER_ROUTES.iter().copied())
        .collect()
}

pub fn build_app(store: AppStore, http: HttpSettings) -> Router {
    let state = AppState { store };
    let docs = Router::new()
        .merge(
            SwaggerUi::new("/docs")
                .url("/openapi.json", openapi::document().clone())
                .config(SwaggerConfig::new(["../openapi.json"])),
        )
        .layer(middleware::from_fn(no_store));

    let mut router = Router::new();
    for (path, method_router) in routes() {
        router = router.route(path, method_router);
    }

    router
        .merge(docs)
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
        .layer(DefaultBodyLimit::max(http.max_body_bytes))
        .layer(middleware::from_fn_with_state(
            http.max_body_bytes,
            reject_large_body,
        ))
        .layer(middleware::from_fn_with_state(
            http.request_timeout,
            request_timeout_middleware,
        ))
        .layer(middleware::from_fn(relative_swagger_redirect))
        .layer(middleware::from_fn(observability::log_request))
        .layer(middleware::from_fn(observability::assign_request_id))
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn relative_swagger_redirect(request: Request, next: Next) -> Response {
    if request.method() == Method::GET && request.uri().path() == "/docs" {
        let mut response = Redirect::to("docs/").into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }
    next.run(request).await
}

async fn request_timeout_middleware(
    State(timeout): State<Duration>,
    request: Request,
    next: Next,
) -> Response {
    match tokio::time::timeout(timeout, next.run(request)).await {
        Ok(response) => response,
        Err(_) => ApiError::timeout().into_response(),
    }
}

/// Recusa pelo `Content-Length` antes de qualquer handler; corpos sem `Content-Length`
/// são limitados pelo `DefaultBodyLimit` nos extractors (`ApiJson` → 413).
async fn reject_large_body(State(max): State<usize>, request: Request, next: Next) -> Response {
    let too_large = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > max as u64);
    if too_large {
        return ApiError::payload_too_large().into_response();
    }
    next.run(request).await
}

async fn not_found() -> ApiError {
    ApiError::route_not_found()
}

async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed()
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::StatusCode};
    use tower::ServiceExt as _;

    use super::*;

    #[tokio::test]
    async fn slow_requests_end_with_the_timeout_envelope() {
        async fn slow() -> &'static str {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "tarde demais"
        }
        let app = Router::new()
            .route("/slow", get(slow))
            .layer(middleware::from_fn_with_state(
                Duration::from_millis(20),
                request_timeout_middleware,
            ));
        let response = app
            .oneshot(Request::get("/slow").body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("body")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["error"]["code"], "REQUEST_TIMEOUT");
    }
}
