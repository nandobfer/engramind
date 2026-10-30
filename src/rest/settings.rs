//! Configurações.

use axum::{Json, extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::settings::{GetSettingsQuery, SettingsView, UpdateSettingsBody},
    error::{ApiError, ErrorResponse},
    http::{ApiJson, ApiQuery},
};

#[utoipa::path(
    get,
    path = "/v1/settings",
    operation_id = "get_settings",
    tag = "Settings",
    summary = "Lê as configurações com valores padrão",
    description = "Cada chave vem com o valor efetivo, o padrão, o tipo, o intervalo e se aceita sobreposição por canal. `key` limita a uma chave. Exige token admin com `read`.",
    params(GetSettingsQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Configurações", body = SettingsView),
        (status = 400, description = "`SETTING_UNKNOWN`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_settings(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<GetSettingsQuery>,
) -> Result<Json<SettingsView>, ApiError> {
    state.core.get_settings(&principal, query).await.map(Json)
}

#[utoipa::path(
    put,
    path = "/v1/settings",
    operation_id = "update_settings",
    tag = "Settings",
    summary = "Altera uma ou mais configurações",
    description = "Recebe um mapa parcial `chave → valor`; `null` volta ao padrão. Tudo ou nada: uma chave inválida recusa o pedido inteiro. Cada mudança gera auditoria. Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = UpdateSettingsBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Configurações depois da mudança", body = SettingsView),
        (status = 400, description = "`VALIDATION_ERROR`, `SETTING_UNKNOWN` ou `SETTING_INVALID`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn update_settings(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<UpdateSettingsBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .update_settings(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}
