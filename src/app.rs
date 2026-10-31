use std::time::Duration;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, Method, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{MethodRouter, get, patch, post, put},
};
use utoipa_swagger_ui::{Config as SwaggerConfig, SwaggerUi};

use crate::{
    auth,
    config::Settings,
    core::Core,
    error::ApiError,
    health,
    mcp::{self, McpSettings},
    observability, openapi, rest,
    store::AppStore,
};

#[derive(Clone)]
pub struct AppState {
    pub store: AppStore,
    pub core: Core<AppStore>,
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

/// Rota do MCP (serviço `rmcp`, fora de `routes()`), também documentada no OpenAPI.
pub const MCP_ROUTE: &str = "/mcp";

/// Rotas públicas, sem token.
fn routes() -> Vec<(&'static str, MethodRouter<AppState>)> {
    vec![
        ("/", get(openapi::root_docs_redirect)),
        ("/health/live", get(health::live)),
        ("/health/ready", get(health::ready)),
    ]
}

/// Rotas `/v1`, protegidas por `require_auth`. Uma entrada por operação do catálogo.
fn protected_routes() -> Vec<(&'static str, MethodRouter<AppState>)> {
    use rest::{
        audit, channels, context, docs, embeddings, identities, jobs, memories, search, settings,
        status, subscriptions,
    };
    vec![
        ("/v1/whoami", get(identities::whoami)),
        (
            "/v1/apps",
            get(identities::list_apps).post(identities::create_app),
        ),
        (
            "/v1/apps/{app}",
            get(identities::get_app)
                .patch(identities::update_app)
                .delete(identities::delete_app),
        ),
        (
            "/v1/agents",
            get(identities::list_agents).post(identities::create_agent),
        ),
        (
            "/v1/agents/{agent}",
            get(identities::get_agent)
                .patch(identities::update_agent)
                .delete(identities::delete_agent),
        ),
        (
            "/v1/tokens",
            get(identities::list_tokens).post(identities::issue_token),
        ),
        ("/v1/tokens/{token}", patch(identities::update_token)),
        ("/v1/tokens/{token}/revoke", post(identities::revoke_token)),
        (
            "/v1/channels",
            get(channels::list_channels).post(channels::create_channel),
        ),
        (
            "/v1/channels/{channel}",
            get(channels::get_channel)
                .patch(channels::update_channel)
                .delete(channels::delete_channel),
        ),
        (
            "/v1/channels/{channel}/archive",
            post(channels::archive_channel),
        ),
        (
            "/v1/channels/{channel}/restore",
            post(channels::restore_channel),
        ),
        ("/v1/channels/{channel}/stats", get(channels::channel_stats)),
        (
            "/v1/channels/{channel}/settings",
            get(settings::get_channel_settings).put(settings::update_channel_settings),
        ),
        ("/v1/context", get(context::get_context)),
        (
            "/v1/memories",
            get(memories::list_memories).post(memories::create_memory),
        ),
        ("/v1/memories/search", post(search::search_memories)),
        ("/v1/hints", post(search::get_hints)),
        (
            "/v1/memories/{memory}",
            get(memories::get_memory)
                .patch(memories::update_memory)
                .delete(memories::delete_memory),
        ),
        (
            "/v1/memories/{memory}/history",
            get(memories::memory_history),
        ),
        (
            "/v1/memories/{memory}/archive",
            post(memories::archive_memory),
        ),
        (
            "/v1/memories/{memory}/restore",
            post(memories::restore_memory),
        ),
        ("/v1/memories/{memory}/pin", post(memories::pin_memory)),
        ("/v1/memories/{memory}/unpin", post(memories::unpin_memory)),
        ("/v1/memories/{memory}/move", post(memories::move_memory)),
        (
            "/v1/memories/{memory}/revert",
            post(memories::revert_memory),
        ),
        (
            "/v1/subscriptions",
            get(subscriptions::list_subscriptions).post(subscriptions::subscribe),
        ),
        (
            "/v1/subscriptions/{subscription}",
            patch(subscriptions::update_subscription).delete(subscriptions::unsubscribe),
        ),
        (
            "/v1/settings",
            get(settings::get_settings).put(settings::update_settings),
        ),
        ("/v1/secrets/{key}", put(settings::set_secret)),
        ("/v1/settings/redator/test", post(settings::test_redator)),
        (
            "/v1/settings/embedding/test",
            post(embeddings::test_embedding),
        ),
        (
            "/v1/settings/embedding/models",
            post(embeddings::list_embedding_models),
        ),
        ("/v1/embeddings/reembed", post(embeddings::reembed_all)),
        ("/v1/jobs", get(jobs::list_jobs)),
        ("/v1/jobs/{job}/retry", post(jobs::retry_job)),
        ("/v1/llm-calls", get(jobs::list_llm_calls)),
        ("/v1/status", get(status::get_status)),
        ("/v1/audit", get(audit::get_audit)),
        ("/v1/docs/overview", get(docs::get_overview)),
        ("/v1/docs/guides", get(docs::list_guides)),
        ("/v1/docs/guides/{guide}", get(docs::get_guide)),
        ("/v1/docs/search", get(docs::search_docs)),
        ("/v1/docs/operations", get(docs::list_operations)),
        ("/v1/docs/operations/{operation}", get(docs::get_operation)),
        ("/v1/docs/schemas/{schema}", get(docs::get_schema)),
    ]
}

/// Caminhos de todas as rotas do router (exceto os ativos internos da Swagger UI).
pub fn route_paths() -> Vec<&'static str> {
    routes()
        .into_iter()
        .chain(protected_routes())
        .map(|(path, _)| path)
        .chain(SWAGGER_ROUTES.iter().copied())
        .chain([MCP_ROUTE])
        .collect()
}

pub fn build_app(core: Core<AppStore>, http: HttpSettings, mcp_settings: &McpSettings) -> Router {
    let state = AppState {
        store: core.store().clone(),
        core: core.clone(),
    };
    let docs = Router::new()
        .merge(
            SwaggerUi::new("/docs")
                .url("/openapi.json", openapi::document().clone())
                .config(SwaggerConfig::new(["../openapi.json"])),
        )
        .layer(middleware::from_fn(no_store));

    let mut protected = Router::new();
    for (path, method_router) in protected_routes() {
        protected = protected.route(path, method_router);
    }
    let protected = protected.route_layer(middleware::from_fn_with_state(
        state.clone(),
        auth::require_auth,
    ));

    let mcp = Router::new()
        .route_service(
            MCP_ROUTE,
            mcp::service(core, mcp_settings, http.max_body_bytes),
        )
        .layer(middleware::from_fn(
            mcp::contract::normalize_transport_errors,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    let mut router = Router::new();
    for (path, method_router) in routes() {
        router = router.route(path, method_router);
    }

    router
        .merge(protected)
        .merge(mcp)
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
