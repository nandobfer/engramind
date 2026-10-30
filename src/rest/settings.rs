//! Configurações.

use axum::{Json, extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::settings::{
        ChannelSettingsView, GetSettingsQuery, SettingsView, UpdateChannelSettingsBody,
        UpdateSettingsBody,
    },
    error::{ApiError, ErrorResponse},
    http::{ApiJson, ApiPath, ApiQuery},
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

#[utoipa::path(
    get,
    path = "/v1/channels/{channel}/settings",
    operation_id = "get_channel_settings",
    tag = "Settings",
    summary = "Lê as configurações de um canal com as sobreposições",
    description = "As chaves que aceitam sobreposição por canal, cada uma com o valor efetivo no canal, o global e a sobreposição (`override`, `null` quando vale o global). Exige token admin com `read`.",
    params(("channel" = String, Path, description = "Id (`ch_…`) ou slug do canal")),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Configurações do canal", body = ChannelSettingsView),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_channel_settings(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiPath(channel): ApiPath<String>,
) -> Result<Json<ChannelSettingsView>, ApiError> {
    state
        .core
        .get_channel_settings(&principal, &channel)
        .await
        .map(Json)
}

#[utoipa::path(
    put,
    path = "/v1/channels/{channel}/settings",
    operation_id = "update_channel_settings",
    tag = "Settings",
    summary = "Altera as sobreposições de configuração de um canal",
    description = "Mapa parcial `chave → valor`, só com chaves que aceitam sobreposição por canal (as outras dão `SETTING_UNKNOWN`, com as válidas em `details.valid`); `null` remove a sobreposição. Tudo ou nada; incrementa a `version` do canal e gera auditoria. Exige token admin com `write`.",
    params(
        ("channel" = String, Path, description = "Id (`ch_…`) ou slug do canal"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")
    ),
    request_body = UpdateChannelSettingsBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Configurações do canal", body = ChannelSettingsView),
        (status = 400, description = "`VALIDATION_ERROR`, `SETTING_UNKNOWN` ou `SETTING_INVALID`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn update_channel_settings(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(channel): ApiPath<String>,
    ApiJson(body): ApiJson<UpdateChannelSettingsBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .update_channel_settings(&principal, &channel, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}
