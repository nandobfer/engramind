//! Documentação viva: guias e índice do OpenAPI da versão em execução.

use axum::{Json, extract::State};
use serde_json::Value;

use crate::{
    AppState,
    auth::Auth,
    core::docs::{DocText, GetGuideQuery, ListOperationsQuery, SearchDocsQuery},
    error::{ApiError, ErrorResponse},
    http::{ApiPath, ApiQuery},
};

#[utoipa::path(
    get,
    path = "/v1/docs/overview",
    operation_id = "get_overview",
    tag = "Documentation",
    summary = "Visão geral do serviço e dos guias disponíveis ao token",
    description = "Propósito, autenticação, envelopes, erros comuns e a lista dos guias visíveis ao token. Comece por aqui.",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Visão geral em Markdown", body = DocText),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_overview(
    State(state): State<AppState>,
    Auth(principal): Auth,
) -> Result<Json<DocText>, ApiError> {
    state.core.get_overview(&principal).map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/docs/guides",
    operation_id = "list_guides",
    tag = "Documentation",
    summary = "Lista os guias com seções e operações",
    description = "Guias visíveis ao token com público, tags OpenAPI, títulos das seções e as operações que cada um documenta. Guias `admin` ficam ocultos para tokens `agent`.",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "`{ service, guides: [{ id, title, audience, tags, sections, operations }], tags }`", body = Object),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_guides(
    State(state): State<AppState>,
    Auth(principal): Auth,
) -> Result<Json<Value>, ApiError> {
    state.core.list_guides(&principal).map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/docs/guides/{guide}",
    operation_id = "get_guide",
    tag = "Documentation",
    summary = "Lê um guia ou uma seção dele",
    description = "Devolve o guia em Markdown ou só a seção pedida (com as subseções). O título da seção ignora acentos e caixa.",
    params(("guide" = String, Path, description = "Id do guia, por exemplo `tokens-e-escopos`"), GetGuideQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Guia em Markdown", body = DocText),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 404, description = "`GUIDE_NOT_FOUND` ou `SECTION_NOT_FOUND`, com `details.valid`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_guide(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiPath(guide): ApiPath<String>,
    ApiQuery(query): ApiQuery<GetGuideQuery>,
) -> Result<Json<DocText>, ApiError> {
    state.core.get_guide(&principal, &guide, query).map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/docs/search",
    operation_id = "search_docs",
    tag = "Documentation",
    summary = "Busca nos guias e nas operações por palavras-chave",
    description = "Ignora acentos e caixa. Ordena por termos cobertos, depois termos no título, depois ocorrências; `total` conta todos os resultados antes de `limit`.",
    params(SearchDocsQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "`{ total, terms, results: [{ kind, guide|operation, section|method+path, snippet, score, matched_terms }] }`", body = Object),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn search_docs(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<SearchDocsQuery>,
) -> Result<Json<Value>, ApiError> {
    state.core.search_docs(&principal, query).map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/docs/operations",
    operation_id = "list_operations",
    tag = "Documentation",
    summary = "Lista as operações visíveis ao token",
    description = "Filtra pelo guia que as documenta ou por uma tag OpenAPI exata. Operações administrativas ficam ocultas para tokens `agent`.",
    params(ListOperationsQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "`{ operations: [{ operation, method, path, tags, summary, security }] }`", body = Object),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 404, description = "`GUIDE_NOT_FOUND` ou `TAG_NOT_FOUND`, com `details.valid`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_operations(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<ListOperationsQuery>,
) -> Result<Json<Value>, ApiError> {
    state.core.list_operations(&principal, query).map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/docs/operations/{operation}",
    operation_id = "get_operation",
    tag = "Documentation",
    summary = "Contrato completo de uma operação",
    description = "Parâmetros, corpo, respostas por status, segurança e todos os esquemas referenciados de uma operação OpenAPI.",
    params(("operation" = String, Path, description = "`operationId`, igual ao nome base, por exemplo `issue_token`")),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "`{ operation, method, path, definition, components }`", body = Object),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 404, description = "`OPERATION_NOT_FOUND`, com `details.valid`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_operation(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiPath(operation): ApiPath<String>,
) -> Result<Json<Value>, ApiError> {
    state.core.get_operation(&principal, &operation).map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/docs/schemas/{schema}",
    operation_id = "get_schema",
    tag = "Documentation",
    summary = "Esquema OpenAPI com as referências",
    description = "Um esquema de `components.schemas` com os esquemas que ele referencia, transitivamente.",
    params(("schema" = String, Path, description = "Nome do esquema, por exemplo `WhoAmI`")),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "`{ name, schema, components }`", body = Object),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 404, description = "`SCHEMA_NOT_FOUND`, com `details.valid`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_schema(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiPath(schema): ApiPath<String>,
) -> Result<Json<Value>, ApiError> {
    state.core.get_schema(&principal, &schema).map(Json)
}
