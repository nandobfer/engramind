//! Auditoria.

use axum::{Json, extract::State};

use crate::{
    AppState,
    auth::Auth,
    core::audit::{AuditList, AuditQuery},
    error::{ApiError, ErrorResponse},
    http::ApiQuery,
};

#[utoipa::path(
    get,
    path = "/v1/audit",
    operation_id = "get_audit",
    tag = "Audit",
    summary = "Lista os eventos de auditoria, do mais recente ao mais antigo",
    description = "Toda mutação de identidade, token e configuração grava um evento com autor, ação, entidade e os estados antes e depois, sem segredos. Filtra por `entity`, `entity_id` e `action`. Exige token admin com `read`.",
    params(AuditQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Página de eventos", body = AuditList),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_audit(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<AuditQuery>,
) -> Result<Json<AuditList>, ApiError> {
    state.core.get_audit(&principal, query).await.map(Json)
}
