//! Contexto: `GET /v1/context` com `ETag` e `304`.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use super::ChannelsHeader;
use crate::{
    AppState,
    auth::Auth,
    core::context::{ContextOutcome, ContextView, GetContextQuery},
    error::{ApiError, ErrorResponse},
    http::ApiQuery,
};

const CACHE_CONTROL: HeaderValue = HeaderValue::from_static("private, no-cache");
const VARY: HeaderValue = HeaderValue::from_static("Authorization, Engramind-Channels");

fn etag(version: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{version}\"")).unwrap_or(HeaderValue::from_static("\"\""))
}

#[utoipa::path(
    get,
    path = "/v1/context",
    operation_id = "get_context",
    tag = "Context",
    summary = "Bloco de memórias pronto para o prompt",
    description = "Memórias `active` e `permanent` dos canais alcançados em modo `inject` (token `admin`: todos os canais ativos), restritas por `channels` e pelo cabeçalho `Engramind-Channels`, em ordem determinística e em duas faixas: texto completo até `context.budget_tokens`, depois `#id: título` até `context.titles_budget_tokens`; o resto é contado em `counts.overflow`. `format`: `tagged` (padrão `context.default_format`), `markdown` (sem ids) ou `json` (`content` é um objeto). O `ETag` é a `version`; com `If-None-Match` igual, responde `304` sem corpo e sem ler o banco.",
    params(
        GetContextQuery,
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula"),
        ("If-None-Match" = Option<String>, Header, description = "`version` (ou ETag) já em uso; igual à atual → `304`")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Contexto renderizado", body = ContextView,
            headers(("ETag" = String, description = "A `version` entre aspas"))),
        (status = 304, description = "Sem mudança desde a `version` em `If-None-Match`",
            headers(("ETag" = String, description = "A `version` entre aspas"))),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_context(
    State(state): State<AppState>,
    Auth(principal): Auth,
    channels: ChannelsHeader,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<GetContextQuery>,
) -> Result<Response, ApiError> {
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok());
    let outcome = state
        .core
        .get_context(&principal, query, if_none_match, channels.as_deref())
        .await?;
    let (version, mut response) = match outcome {
        ContextOutcome::NotModified(not_modified) => (
            not_modified.version,
            StatusCode::NOT_MODIFIED.into_response(),
        ),
        ContextOutcome::Rendered(rendered) => (
            rendered.view.version.clone(),
            Json::<&ContextView>(&rendered.view).into_response(),
        ),
    };
    let headers = response.headers_mut();
    headers.insert(header::ETAG, etag(&version));
    headers.insert(header::CACHE_CONTROL, CACHE_CONTROL);
    headers.insert(header::VARY, VARY);
    Ok(response)
}
