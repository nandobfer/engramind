//! Embeddings: teste, listagem de modelos e reindexação.

use axum::{extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::{
        embeddings::{
            EmbeddingModelList, ListEmbeddingModelsBody, TestEmbeddingBody, TestEmbeddingResult,
        },
        model::ReindexState,
    },
    error::{ApiError, ErrorResponse},
    http::ApiJson,
};

#[utoipa::path(
    post,
    path = "/v1/settings/embedding/test",
    operation_id = "test_embedding",
    tag = "Embeddings",
    summary = "Testa a configuração de embedding com um texto, sem gravar nada",
    description = "Vetoriza `text` (como memória) com a configuração atual ou com a proposta em `settings` (chaves `embedding.*`, inclusive `embedding.api_key` em claro), sem salvar nada. Devolve provedor, modelo, `model_id`, a dimensão detectada e a latência. No provedor `local`, um modelo ainda não carregado dispara o download para `MODELS_PATH` em segundo plano e devolve `EMBEDDING_MODEL_LOADING` (acompanhe `get_status.embedding.local`). Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = TestEmbeddingBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Resultado do teste", body = TestEmbeddingResult),
        (status = 400, description = "`VALIDATION_ERROR`, `SETTING_UNKNOWN`, `SETTING_INVALID` ou `PROVIDER_NOT_CONFIGURED`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT` ou `EMBEDDING_MODEL_LOADING` (modelo local em download ou carregamento)", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 422, description = "`PROVIDER_REJECTED`: o provedor recusou a requisição; `details.reason` traz o motivo dele", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 502, description = "`PROVIDER_FAILED` ou `EMBEDDING_DIMENSION_MISMATCH`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`PROVIDER_TIMEOUT` ou `REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn test_embedding(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<TestEmbeddingBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .test_embedding(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    post,
    path = "/v1/settings/embedding/models",
    operation_id = "list_embedding_models",
    tag = "Embeddings",
    summary = "Lista os modelos de embedding de um provedor",
    description = "Para a tela do sistema gestor montar o seletor: `local` lista os modelos suportados pelo `fastembed` (com a dimensão), `ollama` chama `GET {base_url}/api/tags` e `openai_compatible` chama `GET {base_url}/models` (OpenRouter, OpenAI, LM Studio…). `gemini` e `off` respondem `PROVIDER_LISTING_UNSUPPORTED`. Sem `base_url`/`api_key` no corpo, valem `embedding.base_url` e `embedding.api_key`; a chave do corpo nunca é gravada. Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = ListEmbeddingModelsBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Modelos do provedor", body = EmbeddingModelList),
        (status = 400, description = "`VALIDATION_ERROR`, `SETTING_INVALID`, `PROVIDER_NOT_CONFIGURED` ou `PROVIDER_LISTING_UNSUPPORTED`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 422, description = "`PROVIDER_REJECTED`: o provedor recusou a requisição; `details.reason` traz o motivo dele", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 502, description = "`PROVIDER_FAILED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`PROVIDER_TIMEOUT` ou `REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_embedding_models(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<ListEmbeddingModelsBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .list_embedding_models(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    post,
    path = "/v1/embeddings/reembed",
    operation_id = "reembed_all",
    tag = "Embeddings",
    summary = "Revetoriza todas as memórias com o modelo vigente",
    description = "Marca todas as memórias como `pending` e enfileira jobs `embed` em lotes de `embedding.batch_size`, na mesma transação. O progresso aparece em `get_status.embedding` (`state = reindexing`, `total`, `done`); ao terminar sem falhas, os vetores de outros modelos e os modelos locais fora de uso são apagados. Durante a reindexação, a busca usa só os vetores já gravados do modelo vigente e o FTS. Útil depois de trocar a chave do provedor ou quando um lote falhou de vez (`state = failed`). Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h")),
    security(("bearerAuth" = [])),
    responses(
        (status = 202, description = "Reindexação enfileirada", body = ReindexState),
        (status = 400, description = "`PROVIDER_NOT_CONFIGURED` ou `SETTING_INVALID`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 409, description = "`REEMBED_IN_PROGRESS` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn reembed_all(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
) -> Result<Response, ApiError> {
    let result = state.core.reembed_all(&principal, key.as_deref()).await?;
    Ok(mutation(StatusCode::ACCEPTED, result))
}
