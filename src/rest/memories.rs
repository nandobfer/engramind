//! Memórias.

use axum::{Json, extract::State, http::StatusCode, response::Response};

use super::{ChannelsHeader, IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::{
        memories::{
            CreateMemoryBody, HistoryQuery, ListMemoriesQuery, MemoryHistory, MemoryList,
            MoveMemoryBody, RevertMemoryBody, UpdateMemoryBody,
        },
        model::{Memory, MemoryWrite},
    },
    error::{ApiError, ErrorResponse},
    http::{ApiJson, ApiPath, ApiQuery},
};

#[utoipa::path(
    get,
    path = "/v1/memories",
    operation_id = "list_memories",
    tag = "Memories",
    summary = "Lista as memórias dos canais alcançados",
    description = "Memórias de todos os canais que o token alcança (qualquer modo; canais arquivados só citados pelo slug), restritas por `channels` e pelo cabeçalho `Engramind-Channels`. Padrão `status=active,permanent`. `author` aceita app (`app_…`/slug) ou agente (`ag_…`/`app/agente`); `since` filtra por `updated_at`. `fields=title` devolve só `id`, `title`, `channel`, `category` e `status`. Da alteração mais recente à mais antiga, paginado por `cursor` e `limit`.",
    params(
        ListMemoriesQuery,
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Página de memórias", body = MemoryList),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_memories(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    ApiQuery(query): ApiQuery<ListMemoriesQuery>,
) -> Result<Json<MemoryList>, ApiError> {
    state
        .core
        .list_memories(&principal, query, header.as_deref())
        .await
        .map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/memories/{memory}",
    operation_id = "get_memory",
    tag = "Memories",
    summary = "Mostra uma memória",
    description = "Texto, título, autoria (`app`, `agent`), `origin`, contadores, estados e `version`. Memória num canal fora do alcance do token ou do `Engramind-Channels` responde `MEMORY_NOT_FOUND`.",
    params(
        ("memory" = String, Path, description = "Id da memória (`m_…`)"),
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Memória", body = Memory),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`MEMORY_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_memory(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    ApiPath(memory): ApiPath<String>,
) -> Result<Json<Memory>, ApiError> {
    state
        .core
        .get_memory(&principal, &memory, header.as_deref())
        .await
        .map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/memories/{memory}/history",
    operation_id = "memory_history",
    tag = "Memories",
    summary = "Lista as revisões de uma memória",
    description = "Uma revisão por mudança (`create`, `update`, `archive`, `restore`, `pin`, `unpin`, `move`, `revert`), com texto, título, categoria, status, autor e motivo, da versão mais nova à mais antiga.",
    params(
        ("memory" = String, Path, description = "Id da memória (`m_…`)"),
        HistoryQuery,
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Revisões", body = MemoryHistory),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`MEMORY_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn memory_history(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    ApiPath(memory): ApiPath<String>,
    ApiQuery(query): ApiQuery<HistoryQuery>,
) -> Result<Json<MemoryHistory>, ApiError> {
    state
        .core
        .memory_history(&principal, &memory, query, header.as_deref())
        .await
        .map(Json)
}

#[utoipa::path(
    post,
    path = "/v1/memories",
    operation_id = "create_memory",
    tag = "Memories",
    summary = "Grava uma memória num canal",
    description = "Exige `write` efetivo no canal. Grava na hora com título provisório (`title_state: pending`), a revisão 1 e a auditoria, e enfileira o Redator e o embedding; nunca espera o LLM. Texto igual (normalizado) a outra memória do canal devolve a existente com `duplicate: true` e `200`, sem gravar. `similar` traz até três memórias parecidas do canal: prefira `update_memory` a uma quase-duplicata. Aceita `Idempotency-Key`.",
    params(
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h"),
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    request_body = CreateMemoryBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 201, description = "Memória criada", body = MemoryWrite),
        (status = 200, description = "Duplicata: a memória existente, com `duplicate: true`", body = MemoryWrite),
        (status = 400, description = "`VALIDATION_ERROR` ou `CATEGORY_NOT_FOUND`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED` ou `CHANNEL_NOT_SUBSCRIBED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`CHANNEL_ARCHIVED` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn create_memory(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<CreateMemoryBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .create_memory(&principal, body, header.as_deref(), key.as_deref())
        .await?;
    let status = if result.value.duplicate {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok(mutation(status, result))
}

#[utoipa::path(
    patch,
    path = "/v1/memories/{memory}",
    operation_id = "update_memory",
    tag = "Memories",
    summary = "Edita o texto ou a categoria de uma memória",
    description = "Exige `write` efetivo no canal. Com `expected_version`, uma versão diferente responde `409 MEMORY_VERSION_CONFLICT` com `details.current_version`. Texto novo volta o título e o embedding para `pending`, zera `importance` e reenfileira o Redator; texto igual a outra memória do canal responde `409 MEMORY_DUPLICATE` com `details.existing_id`. Sem mudança efetiva, nada é gravado. Aceita `Idempotency-Key`.",
    params(
        ("memory" = String, Path, description = "Id da memória (`m_…`)"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h"),
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    request_body = UpdateMemoryBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Memória editada", body = MemoryWrite),
        (status = 400, description = "`VALIDATION_ERROR` ou `CATEGORY_NOT_FOUND`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`MEMORY_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`MEMORY_VERSION_CONFLICT`, `MEMORY_DUPLICATE`, `CHANNEL_ARCHIVED` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn update_memory(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    key: IdempotencyKey,
    ApiPath(memory): ApiPath<String>,
    ApiJson(body): ApiJson<UpdateMemoryBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .update_memory(&principal, &memory, body, header.as_deref(), key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

macro_rules! status_route {
    ($name:ident, $path:literal, $summary:literal, $description:literal, $ok:literal) => {
        #[utoipa::path(
            post,
            path = $path,
            operation_id = stringify!($name),
            tag = "Memories",
            summary = $summary,
            description = $description,
            params(
                ("memory" = String, Path, description = "Id da memória (`m_…`)"),
                ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h"),
                ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
            ),
            security(("bearerAuth" = [])),
            responses(
                (status = 200, description = $ok, body = Memory),
                (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
                (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
                (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
                (status = 404, description = "`MEMORY_NOT_FOUND`", body = ErrorResponse),
                (status = 409, description = "`CHANNEL_ARCHIVED` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
                (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
                (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
                (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
            )
        )]
        pub async fn $name(
            State(state): State<AppState>,
            Auth(principal): Auth,
            header: ChannelsHeader,
            key: IdempotencyKey,
            ApiPath(memory): ApiPath<String>,
        ) -> Result<Response, ApiError> {
            let result = state
                .core
                .$name(&principal, &memory, header.as_deref(), key.as_deref())
                .await?;
            Ok(mutation(StatusCode::OK, result))
        }
    };
}

status_route!(
    archive_memory,
    "/v1/memories/{memory}/archive",
    "Arquiva uma memória",
    "Exige `write` efetivo no canal. A memória sai do contexto e das dicas e continua buscável com `include_archived`. Arquivar uma memória arquivada não muda nada. Aceita `Idempotency-Key`.",
    "Memória arquivada"
);
status_route!(
    restore_memory,
    "/v1/memories/{memory}/restore",
    "Restaura uma memória arquivada",
    "Exige `write` efetivo no canal. A memória arquivada volta a `active`; nos outros status, nada muda. Aceita `Idempotency-Key`.",
    "Memória restaurada"
);
status_route!(
    pin_memory,
    "/v1/memories/{memory}/pin",
    "Fixa uma memória (status permanent)",
    "Exige `write` efetivo no canal. A memória fica `permanent`: entra primeiro no contexto e o consolidador nunca a toca. Fixar uma memória fixada não muda nada. Aceita `Idempotency-Key`.",
    "Memória fixada"
);
status_route!(
    unpin_memory,
    "/v1/memories/{memory}/unpin",
    "Desafixa uma memória",
    "Exige `write` efetivo no canal. A memória `permanent` volta a `active`; nos outros status, nada muda. Aceita `Idempotency-Key`.",
    "Memória desafixada"
);

#[utoipa::path(
    post,
    path = "/v1/memories/{memory}/move",
    operation_id = "move_memory",
    tag = "Memories",
    summary = "Move uma memória para outro canal",
    description = "Exige `write` efetivo na origem e no destino. Texto igual a outra memória do destino responde `409 MEMORY_DUPLICATE` com `details.existing_id`. Incrementa a versão da memória e dos dois canais. Aceita `Idempotency-Key`.",
    params(
        ("memory" = String, Path, description = "Id da memória (`m_…`)"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h"),
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    request_body = MoveMemoryBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Memória movida", body = Memory),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED` ou `CHANNEL_NOT_SUBSCRIBED`", body = ErrorResponse),
        (status = 404, description = "`MEMORY_NOT_FOUND` ou `CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`MEMORY_DUPLICATE`, `CHANNEL_ARCHIVED` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn move_memory(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    key: IdempotencyKey,
    ApiPath(memory): ApiPath<String>,
    ApiJson(body): ApiJson<MoveMemoryBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .move_memory(&principal, &memory, body, header.as_deref(), key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    post,
    path = "/v1/memories/{memory}/revert",
    operation_id = "revert_memory",
    tag = "Memories",
    summary = "Restaura o conteúdo de uma versão anterior numa revisão nova",
    description = "Exige `write` efetivo no canal. Texto, título e categoria da versão pedida voltam numa revisão nova; o histórico nunca é reescrito. O Redator só refaz o título (o texto restaurado fica como está). Conteúdo igual ao atual não muda nada. Aceita `Idempotency-Key`.",
    params(
        ("memory" = String, Path, description = "Id da memória (`m_…`)"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h"),
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    request_body = RevertMemoryBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Memória revertida", body = Memory),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`MEMORY_NOT_FOUND` ou `REVISION_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`MEMORY_VERSION_CONFLICT`, `MEMORY_DUPLICATE`, `CHANNEL_ARCHIVED` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn revert_memory(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    key: IdempotencyKey,
    ApiPath(memory): ApiPath<String>,
    ApiJson(body): ApiJson<RevertMemoryBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .revert_memory(&principal, &memory, body, header.as_deref(), key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    delete,
    path = "/v1/memories/{memory}",
    operation_id = "delete_memory",
    tag = "Memories",
    summary = "Exclui uma memória, mantendo a última revisão",
    description = "Exige `delete` efetivo no canal. Apaga a memória e o índice de busca; a auditoria e a última revisão guardam a proveniência. Devolve a memória como estava. Aceita `Idempotency-Key`.",
    params(
        ("memory" = String, Path, description = "Id da memória (`m_…`)"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h"),
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Memória excluída", body = Memory),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`MEMORY_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn delete_memory(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    key: IdempotencyKey,
    ApiPath(memory): ApiPath<String>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .delete_memory(&principal, &memory, header.as_deref(), key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}
