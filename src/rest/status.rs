//! Status e uso.

use axum::{Json, extract::State};

use crate::{
    AppState,
    auth::Auth,
    core::{
        status::StatusView,
        usage_report::{GetUsageQuery, UsageReport},
    },
    error::{ApiError, ErrorResponse},
    http::ApiQuery,
};

#[utoipa::path(
    get,
    path = "/v1/status",
    operation_id = "get_status",
    tag = "Status",
    summary = "Mostra versão, uptime, banco, backup, fila, Redator, embeddings e webhooks",
    description = "Versão em execução, uptime, banco (migrações e tamanho), idade da última réplica do Litestream (medida a cada 60 s), contagens de apps, agentes e tokens, fila por tipo e estado, Redator (configuração, últimos resultados e as últimas 24 h), embeddings, webhooks (apps, pendentes, entregues e falhas em 24 h) e a última retenção. Exige token admin com `read`.",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Status", body = StatusView),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_status(
    State(state): State<AppState>,
    Auth(principal): Auth,
) -> Result<Json<StatusView>, ApiError> {
    state.core.get_status(&principal).await.map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/usage",
    operation_id = "get_usage",
    tag = "Status",
    summary = "Agrega o uso (contexto, busca e dicas) por dia, agente, operação ou canal",
    description = "Soma `usage_log` (`get_context` entregue, `search_memories` e `get_hints`) em `[since, until)`: requisições, memórias e tokens renderizados. Padrão: os últimos 7 dias por `day` (UTC); no máximo 366 dias. Por `channel`, uma requisição conta em cada canal que alcançou e as memórias no canal atual delas; o `total` conta cada requisição uma vez. Exige token admin com `read`.",
    params(GetUsageQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Uso agregado", body = UsageReport),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_usage(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<GetUsageQuery>,
) -> Result<Json<UsageReport>, ApiError> {
    state.core.get_usage(&principal, query).await.map(Json)
}
