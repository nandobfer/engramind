//! Consolidação (009).

use axum::{extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::consolidation::{ConsolidationRun, RunConsolidationBody},
    error::{ApiError, ErrorResponse},
    http::ApiJson,
};

#[utoipa::path(
    post,
    path = "/v1/consolidation/run",
    operation_id = "run_consolidation",
    tag = "Consolidation",
    summary = "Consolida canais sob demanda ou mostra o plano (dry_run)",
    description = "Com `dry_run: true`, devolve na hora (`200`), sem gravar e sem chamar o LLM, o plano de cada canal: pares candidatos a fusão (com o cosseno), memórias a classificar, sinalizações, limpezas e arquivamentos de desuso e as tarefas puladas com o motivo; a execução avalia exatamente esses candidatos. Sem `dry_run`, enfileira um job `consolidate` por canal (`202`); canal com um job pendente devolve esse com `already_queued: true`. `channel` ausente consolida todos os canais com `consolidation.enabled`; `channel` explícito desligado responde `409 CONSOLIDATION_DISABLED` (o `dry_run` é permitido). `tasks` padrão = `consolidation.tasks`. Aceita `Idempotency-Key` (sem efeito no `dry_run`). Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = RunConsolidationBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Plano (`dry_run`)", body = ConsolidationRun),
        (status = 202, description = "Jobs `consolidate` enfileirados", body = ConsolidationRun),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`CONSOLIDATION_DISABLED`, `CHANNEL_ARCHIVED` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn run_consolidation(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<RunConsolidationBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .run_consolidation(&principal, body, key.as_deref())
        .await?;
    let status = if result.value.dry_run {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    Ok(mutation(status, result))
}
