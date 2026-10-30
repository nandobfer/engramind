//! Canais.

use axum::{Json, extract::State, http::StatusCode, response::Response};

use super::{ChannelsHeader, IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::{
        channels::{ChannelList, CreateChannelBody, ListChannelsQuery, UpdateChannelBody},
        model::{Channel, ChannelStats},
    },
    error::{ApiError, ErrorResponse},
    http::{ApiJson, ApiPath, ApiQuery},
};

#[utoipa::path(
    get,
    path = "/v1/channels",
    operation_id = "list_channels",
    tag = "Channels",
    summary = "Lista os canais alcançados pelo token",
    description = "Token `agent` vê só os canais que assina (inclusive os de modo `manage`), cada um com o `reach` efetivo (`mode`, `access`, `priority` e a assinatura mais específica); token `admin` vê todos, em modo `inject`, com os próprios escopos. `channels` e o cabeçalho `Engramind-Channels` restringem (com os dois, vale a interseção). Arquivados só com `include_archived=true`. Em ordem de slug, paginado por `cursor` e `limit`.",
    params(
        ListChannelsQuery,
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Página de canais", body = ChannelList),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_channels(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    ApiQuery(query): ApiQuery<ListChannelsQuery>,
) -> Result<Json<ChannelList>, ApiError> {
    state
        .core
        .list_channels(&principal, query, header.as_deref())
        .await
        .map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/channels/{channel}",
    operation_id = "get_channel",
    tag = "Channels",
    summary = "Mostra um canal alcançado pelo token",
    description = "`{channel}` aceita o id (`ch_…`) ou o slug com `/` codificada como `%2F` (`eng%2Fglobal`). Canal arquivado continua legível. Token `agent` sem assinatura no canal, ou canal fora do `Engramind-Channels`, recebe `CHANNEL_NOT_SUBSCRIBED`.",
    params(
        ("channel" = String, Path, description = "Id (`ch_…`) ou slug do canal"),
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Canal com o alcance do token", body = Channel),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED` ou `CHANNEL_NOT_SUBSCRIBED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_channel(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    ApiPath(channel): ApiPath<String>,
) -> Result<Json<Channel>, ApiError> {
    state
        .core
        .get_channel(&principal, &channel, header.as_deref())
        .await
        .map(Json)
}

#[utoipa::path(
    post,
    path = "/v1/channels",
    operation_id = "create_channel",
    tag = "Network",
    summary = "Cria um canal",
    description = "O slug é hierárquico (`eng/global`), único e imutável. Assinaturas por prefixo passam a alcançar o canal novo na hora. Aceita `Idempotency-Key`. Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = CreateChannelBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 201, description = "Canal criado", body = Channel),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 409, description = "`SLUG_ALREADY_EXISTS` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn create_channel(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<CreateChannelBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .create_channel(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::CREATED, result))
}

#[utoipa::path(
    patch,
    path = "/v1/channels/{channel}",
    operation_id = "update_channel",
    tag = "Network",
    summary = "Altera nome, descrição ou prioridade de um canal",
    description = "Campos omitidos não mudam; `description: null` limpa. O slug não muda. Incrementa `version`. Exige token admin com `write`.",
    params(
        ("channel" = String, Path, description = "Id (`ch_…`) ou slug do canal"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")
    ),
    request_body = UpdateChannelBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Canal alterado", body = Channel),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn update_channel(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(channel): ApiPath<String>,
    ApiJson(body): ApiJson<UpdateChannelBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .update_channel(&principal, &channel, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    post,
    path = "/v1/channels/{channel}/archive",
    operation_id = "archive_channel",
    tag = "Network",
    summary = "Arquiva um canal",
    description = "O canal sai do contexto, da busca e das dicas padrão e de `list_channels` sem `include_archived`; continua legível quando citado pelo slug. Arquivar um canal já arquivado não muda nada. Exige token admin com `write`.",
    params(
        ("channel" = String, Path, description = "Id (`ch_…`) ou slug do canal"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Canal arquivado", body = Channel),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn archive_channel(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(channel): ApiPath<String>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .archive_channel(&principal, &channel, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    post,
    path = "/v1/channels/{channel}/restore",
    operation_id = "restore_channel",
    tag = "Network",
    summary = "Restaura um canal arquivado",
    description = "O canal volta ao contexto, à busca e às listagens. Restaurar um canal ativo não muda nada. Exige token admin com `write`.",
    params(
        ("channel" = String, Path, description = "Id (`ch_…`) ou slug do canal"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Canal restaurado", body = Channel),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn restore_channel(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(channel): ApiPath<String>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .restore_channel(&principal, &channel, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    delete,
    path = "/v1/channels/{channel}",
    operation_id = "delete_channel",
    tag = "Network",
    summary = "Exclui um canal arquivado",
    description = "Só exclui canal arquivado (`CHANNEL_NOT_ARCHIVED`). Remove o canal e as assinaturas exatas dele; as assinaturas por prefixo continuam. A auditoria guarda o canal e quantas assinaturas saíram. Exige token admin com `delete`.",
    params(
        ("channel" = String, Path, description = "Id (`ch_…`) ou slug do canal"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Canal excluído (estado anterior)", body = Channel),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`CHANNEL_NOT_ARCHIVED` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn delete_channel(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(channel): ApiPath<String>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .delete_channel(&principal, &channel, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    get,
    path = "/v1/channels/{channel}/stats",
    operation_id = "channel_stats",
    tag = "Channels",
    summary = "Contagens das memórias de um canal",
    description = "Total e por status, por categoria, pendências de título e embedding, soma de `retrieval_count`, a última mudança e a `version` do canal. Exige `read` efetivo no canal.",
    params(
        ("channel" = String, Path, description = "Id (`ch_…`) ou slug do canal"),
        ("Engramind-Channels" = Option<String>, Header, description = "Filtro de canais da conexão: slugs e prefixos separados por vírgula")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Estatísticas do canal", body = ChannelStats),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED` ou `CHANNEL_NOT_SUBSCRIBED`", body = ErrorResponse),
        (status = 404, description = "`CHANNEL_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn channel_stats(
    State(state): State<AppState>,
    Auth(principal): Auth,
    header: ChannelsHeader,
    ApiPath(channel): ApiPath<String>,
) -> Result<Json<ChannelStats>, ApiError> {
    state
        .core
        .channel_stats(&principal, &channel, header.as_deref())
        .await
        .map(Json)
}
