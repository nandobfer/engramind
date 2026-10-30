//! Assinaturas.

use axum::{Json, extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::{
        model::Subscription,
        subscriptions::{
            ListSubscriptionsQuery, SubscribeBody, SubscriptionList, UpdateSubscriptionBody,
        },
    },
    error::{ApiError, ErrorResponse},
    http::{ApiJson, ApiPath, ApiQuery},
};

#[utoipa::path(
    get,
    path = "/v1/subscriptions",
    operation_id = "list_subscriptions",
    tag = "Channels",
    summary = "Lista as assinaturas (token agent: só as próprias)",
    description = "Filtra por `agent` (id ou `app/agente`) e por `channel` (as assinaturas exatas ou por prefixo que alcançam o canal). Token `agent` vê só as do próprio agente. Cada assinatura traz `reaches`, os canais ativos que o alvo alcança agora. Em ordem de criação, paginado por `cursor` e `limit`.",
    params(ListSubscriptionsQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Página de assinaturas", body = SubscriptionList),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`AGENT_NOT_FOUND` ou `CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_subscriptions(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<ListSubscriptionsQuery>,
) -> Result<Json<SubscriptionList>, ApiError> {
    state
        .core
        .list_subscriptions(&principal, query)
        .await
        .map(Json)
}

#[utoipa::path(
    post,
    path = "/v1/subscriptions",
    operation_id = "subscribe",
    tag = "Network",
    summary = "Assina um agente a um canal ou prefixo",
    description = "`target` é um canal exato existente (`eng/global`) ou um prefixo terminado em `*` (`eng/*`, `eng/projeto-*`) que vale também para canais criados depois. Com várias assinaturas no mesmo canal, vale a mais específica. `access` só restringe os escopos do token (omitido: os do token). Aceita `Idempotency-Key`. Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = SubscribeBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 201, description = "Assinatura criada", body = Subscription),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`AGENT_NOT_FOUND` ou `CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`SUBSCRIPTION_EXISTS` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn subscribe(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<SubscribeBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .subscribe(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::CREATED, result))
}

#[utoipa::path(
    patch,
    path = "/v1/subscriptions/{subscription}",
    operation_id = "update_subscription",
    tag = "Network",
    summary = "Altera modo, acesso ou prioridade de uma assinatura",
    description = "Campos omitidos não mudam; `access: null` volta aos escopos do token e `priority: null` à prioridade do canal. Agente e alvo não mudam: cancele e assine de novo. Vale na requisição seguinte. Exige token admin com `write`.",
    params(
        ("subscription" = String, Path, description = "Id da assinatura (`sub_…`)"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")
    ),
    request_body = UpdateSubscriptionBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Assinatura alterada", body = Subscription),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`SUBSCRIPTION_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn update_subscription(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(subscription): ApiPath<String>,
    ApiJson(body): ApiJson<UpdateSubscriptionBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .update_subscription(&principal, &subscription, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    delete,
    path = "/v1/subscriptions/{subscription}",
    operation_id = "unsubscribe",
    tag = "Network",
    summary = "Cancela uma assinatura",
    description = "O agente deixa de alcançar os canais da assinatura na requisição seguinte. Exige token admin com `write`.",
    params(
        ("subscription" = String, Path, description = "Id da assinatura (`sub_…`)"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Assinatura cancelada (estado anterior)", body = Subscription),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`SUBSCRIPTION_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn unsubscribe(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(subscription): ApiPath<String>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .unsubscribe(&principal, &subscription, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}
