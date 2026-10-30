use std::time::Duration;

use axum::{Json, extract::State};
use serde::Serialize;
use utoipa::ToSchema;

use crate::{
    AppState,
    error::{ApiError, ErrorResponse},
    store::{MigrationStatus, ReadyReport, Store, StoreError},
};

const READY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Serialize, ToSchema)]
pub struct LiveResponse {
    #[schema(example = "ok")]
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadyResponse {
    #[schema(example = "ready")]
    pub status: String,
    #[schema(example = "ok")]
    pub database: String,
    pub migrations: MigrationStatus,
}

#[utoipa::path(
    get,
    path = "/health/live",
    operation_id = "health_live",
    tag = "Health",
    summary = "Confere se o processo está vivo",
    description = "Responde sem tocar no banco. Usado pelo `HEALTHCHECK` da imagem.",
    security(),
    responses(
        (status = 200, description = "Processo vivo", body = LiveResponse),
        (status = 504, description = "Tempo limite da requisição", body = ErrorResponse)
    )
)]
pub async fn live() -> Json<LiveResponse> {
    Json(LiveResponse {
        status: "ok".to_owned(),
    })
}

#[utoipa::path(
    get,
    path = "/health/ready",
    operation_id = "health_ready",
    tag = "Health",
    summary = "Confere se o serviço pode receber tráfego",
    description = "Confere o acesso ao banco e se todas as migrações embutidas foram aplicadas, sem expor a configuração.",
    security(),
    responses(
        (status = 200, description = "Serviço pronto", body = ReadyResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE` ou `MIGRATIONS_PENDING`", body = ErrorResponse),
        (status = 504, description = "Tempo limite da requisição", body = ErrorResponse)
    )
)]
pub async fn ready(State(state): State<AppState>) -> Result<Json<ReadyResponse>, ApiError> {
    let result = tokio::time::timeout(READY_TIMEOUT, state.store.ready())
        .await
        .unwrap_or(Err(StoreError::Unavailable));
    readiness(result).map(Json)
}

pub fn readiness(result: Result<ReadyReport, StoreError>) -> Result<ReadyResponse, ApiError> {
    let report = result.map_err(|_| ApiError::database_unavailable())?;
    if report.migrations.applied < report.migrations.expected {
        return Err(
            ApiError::migrations_pending().with_details(serde_json::json!({
                "applied": report.migrations.applied,
                "expected": report.migrations.expected,
            })),
        );
    }
    Ok(ReadyResponse {
        status: "ready".to_owned(),
        database: "ok".to_owned(),
        migrations: report.migrations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_maps_store_failures_to_stable_codes() {
        let pending = ReadyReport {
            migrations: MigrationStatus {
                applied: 0,
                expected: 1,
            },
        };
        assert_eq!(
            readiness(Ok(pending)).expect_err("pending").code,
            "MIGRATIONS_PENDING"
        );
        for error in [
            StoreError::Unavailable,
            StoreError::Internal("x".to_owned()),
        ] {
            assert_eq!(
                readiness(Err(error)).expect_err("unavailable").code,
                "DATABASE_UNAVAILABLE"
            );
        }
    }
}
