//! Busca e dicas.

use axum::{Json, extract::State};

use super::ChannelsHeader;
use crate::{
    AppState,
    auth::Auth,
    core::{
        hints::{HintsBody, HintsResult},
        search::{SearchMemoriesBody, SearchResults},
    },
    error::{ApiError, ErrorResponse},
    http::ApiJson,
};

#[utoipa::path(
    post,
    path = "/v1/memories/search",
    operation_id = "search_memories",
    tag = "Search",
    summary = "Busca memórias por sentido e por texto",
    description = "Busca híbrida nos canais em modo `inject` e `search`; canais `manage` e arquivados só entram citados pelo slug em `channels` ou no `Engramind-Channels`. O ramo vetorial compara o embedding da consulta com os vetores do modelo vigente (cosseno do sqlite-vec) e o textual usa FTS5/BM25 sem acentos; a fusão RRF dá `score = Σ peso / (search.rrf_k + posição)` com `search.vector_weight` e `search.text_weight`. Sem embeddings (desligados, sem configuração, provedor com falha ou acima de 2 s), só o FTS e `mode: text`. Padrão `limit = search.default_limit` (máximo 50), só memórias ativas e fixadas (`include_archived` inclui as arquivadas). `exclude_ids` e `exclude_context` tiram o que já está no contexto. Leitura: não aceita `Idempotency-Key`.",
    params(
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    request_body = SearchMemoriesBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Resultados por relevância", body = SearchResults),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn search_memories(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    ApiJson(body): ApiJson<SearchMemoriesBody>,
) -> Result<Json<SearchResults>, ApiError> {
    state
        .core
        .search_memories(&principal, body, header.as_deref())
        .await
        .map(Json)
}

#[utoipa::path(
    post,
    path = "/v1/hints",
    operation_id = "get_hints",
    tag = "Search",
    summary = "Dicas baratas (id, título e canal) para um texto, sem LLM",
    description = "A \"lembrança vaga\": busca vetorial com o limiar `search.hint_threshold` sobre o cosseno, só em canais `inject` e `search` (nunca `manage`, mesmo citado), sem as memórias que já estão com texto completo no contexto do mesmo token e seleção. Devolve até `limit` (padrão `search.hint_limit`, máximo 20) itens `{ id, title, channel }`; o agente busca o texto completo se quiser. Sem embeddings, lista vazia. Feita para o sistema consumidor chamar antes do turno. Leitura: não aceita `Idempotency-Key`.",
    params(
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    request_body = HintsBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Dicas por similaridade", body = HintsResult),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_hints(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    ApiJson(body): ApiJson<HintsBody>,
) -> Result<Json<HintsResult>, ApiError> {
    state
        .core
        .get_hints(&principal, body, header.as_deref())
        .await
        .map(Json)
}
