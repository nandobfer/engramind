//! Configurações.

use axum::{Json, extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::{
        model::SecretView,
        redator::{TestRedatorBody, TestRedatorResult},
        secrets::SetSecretBody,
        settings::{
            ChannelSettingsView, GetSettingsQuery, SettingsView, UpdateChannelSettingsBody,
            UpdateSettingsBody,
        },
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
    description = "Cada chave vem com o valor efetivo, o padrão, o tipo, o intervalo e se aceita sobreposição por canal; os segredos vêm só com `secret: true`, `configured` e `updated_at`. `key` limita a uma chave. Exige token admin com `read`.",
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
    description = "Recebe um mapa parcial `chave → valor`; `null` volta ao padrão. Tudo ou nada: uma chave inválida recusa o pedido inteiro. Segredos dão `SETTING_IS_SECRET` (use `set_secret`). Cada mudança gera auditoria. Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = UpdateSettingsBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Configurações depois da mudança", body = SettingsView),
        (status = 400, description = "`VALIDATION_ERROR`, `SETTING_UNKNOWN`, `SETTING_INVALID` ou `SETTING_IS_SECRET`", body = ErrorResponse),
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

#[utoipa::path(
    put,
    path = "/v1/secrets/{key}",
    operation_id = "set_secret",
    tag = "Settings",
    summary = "Grava ou apaga um segredo de configuração",
    description = "Cifra o valor com AES-256-GCM (`SECRETS_KEY`) e devolve só `{ key, configured, updated_at }`; o valor nunca volta em resposta, log ou auditoria. `value: null` apaga. Chaves válidas: as configurações do tipo `secret` (`redator.api_key`). Exige token admin com `write`.",
    params(
        ("key" = String, Path, description = "Chave do segredo, por exemplo `redator.api_key`"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")
    ),
    request_body = SetSecretBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Estado do segredo", body = SecretView),
        (status = 400, description = "`VALIDATION_ERROR` ou `SECRET_UNKNOWN`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn set_secret(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(name): ApiPath<String>,
    ApiJson(body): ApiJson<SetSecretBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .set_secret(&principal, &name, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    post,
    path = "/v1/settings/redator/test",
    operation_id = "test_redator",
    tag = "Settings",
    summary = "Testa o Redator com um texto, sem gravar a memória",
    description = "Chama o LLM com a configuração atual ou com a proposta em `settings` (chaves `redator.*`, inclusive `redator.api_key` em claro), sem salvar nada além da linha de `llm_calls` (`purpose = test`). Devolve o título, o texto revisado, o modelo que respondeu e a latência. A guarda de sentido não é aplicada. Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = TestRedatorBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Resultado do Redator", body = TestRedatorResult),
        (status = 400, description = "`VALIDATION_ERROR`, `SETTING_UNKNOWN`, `SETTING_INVALID` ou `PROVIDER_NOT_CONFIGURED`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 502, description = "`PROVIDER_FAILED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`PROVIDER_TIMEOUT` ou `REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn test_redator(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<TestRedatorBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .test_redator(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}
