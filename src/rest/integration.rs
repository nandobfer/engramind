//! Integração (008): importação de memórias.

use axum::{extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::{import::ImportMemoriesBody, model::ImportResult},
    error::{ApiError, ErrorResponse},
    http::ApiJson,
};

#[utoipa::path(
    post,
    path = "/v1/memories/import",
    operation_id = "import_memories",
    tag = "Integration",
    summary = "Importa memórias em lote preservando data, autoria e origem",
    description = "Grava até `limits.max_batch` memórias num canal ativo, numa transação: tudo é validado antes (um item inválido recusa o lote, com `details.index`). Preserva `created_at` (não futuro), o autor (`author: { app, agent? }`, que precisa existir; omitido, o token) e `origin`; cria a revisão 1, enfileira o Redator e os embeddings e um único evento `memory.created` com os ids criados. Textos iguais a uma memória do canal (ou a um item anterior do lote) ficam em `duplicates`. Exige `Idempotency-Key` e token admin com `write`.",
    params(("Idempotency-Key" = String, Header, description = "Obrigatória: repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = ImportMemoriesBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 201, description = "Lote importado", body = ImportResult),
        (status = 400, description = "`VALIDATION_ERROR`, `IDEMPOTENCY_KEY_REQUIRED`, `BATCH_TOO_LARGE` ou `CATEGORY_NOT_FOUND`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`, `APP_NOT_FOUND` ou `AGENT_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`CHANNEL_ARCHIVED` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn import_memories(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<ImportMemoriesBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .import_memories(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::CREATED, result))
}
