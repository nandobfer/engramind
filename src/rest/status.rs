//! Status básico.

use axum::{Json, extract::State};

use crate::{
    AppState,
    auth::Auth,
    core::status::StatusView,
    error::{ApiError, ErrorResponse},
};

#[utoipa::path(
    get,
    path = "/v1/status",
    operation_id = "get_status",
    tag = "Status",
    summary = "Mostra versão, banco, backup e contagens",
    description = "Versão em execução, estado do banco e das migrações, idade da última réplica do Litestream (quando habilitado) e contagens de apps, agentes e tokens. Exige token admin com `read`.",
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
