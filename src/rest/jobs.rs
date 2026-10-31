//! Fila de jobs e chamadas ao LLM.

use axum::{Json, extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::{
        jobs::{JobList, ListJobsQuery, ListLlmCallsQuery, LlmCallList},
        model::Job,
    },
    error::{ApiError, ErrorResponse},
    http::{ApiPath, ApiQuery},
};

#[utoipa::path(
    get,
    path = "/v1/jobs",
    operation_id = "list_jobs",
    tag = "Jobs",
    summary = "Lista os jobs da fila, do mais recente ao mais antigo",
    description = "Jobs `redact` (Redator) e `embed` (a partir da 007), com estado, tentativas, próxima execução, último erro e resultado. Filtra por `state` e `kind`. O payload só tem ids e versões. Exige token admin com `read`.",
    params(ListJobsQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Página de jobs", body = JobList),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_jobs(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<ListJobsQuery>,
) -> Result<Json<JobList>, ApiError> {
    state.core.list_jobs(&principal, query).await.map(Json)
}

#[utoipa::path(
    post,
    path = "/v1/jobs/{job}/retry",
    operation_id = "retry_job",
    tag = "Jobs",
    summary = "Recoloca na fila um job que falhou",
    description = "Só jobs com `state = failed`: voltam para `queued` com as tentativas zeradas e execução imediata. Outro estado dá `409 JOB_NOT_RETRYABLE`. Gera auditoria `job.retry`. Exige token admin com `write`.",
    params(
        ("job" = String, Path, description = "Id numérico do job"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Job de volta à fila", body = Job),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`JOB_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`JOB_NOT_RETRYABLE` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn retry_job(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(job): ApiPath<String>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .retry_job(&principal, &job, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    get,
    path = "/v1/llm-calls",
    operation_id = "list_llm_calls",
    tag = "Jobs",
    summary = "Lista as chamadas ao LLM, da mais recente à mais antiga",
    description = "Uma linha por chamada (job `redact` ou `test_redator`): finalidade, provedor, modelo, tokens, custo quando o provedor informa, duração, resultado e erro curto. Nunca o texto, o prompt ou a chave. Filtra por `since` e `purpose`. Exige token admin com `read`.",
    params(ListLlmCallsQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Página de chamadas", body = LlmCallList),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_llm_calls(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<ListLlmCallsQuery>,
) -> Result<Json<LlmCallList>, ApiError> {
    state.core.list_llm_calls(&principal, query).await.map(Json)
}
