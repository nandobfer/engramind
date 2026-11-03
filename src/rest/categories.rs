//! Categorias de memória: `list_categories` e `create_category` (0.11.0).

use axum::{Json, extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::categories::{CategoryList, CategoryWrite, CreateCategoryBody},
    error::{ApiError, ErrorResponse},
    http::ApiJson,
};

#[utoipa::path(
    get,
    path = "/v1/categories",
    operation_id = "list_categories",
    tag = "Settings",
    summary = "Lista as categorias de memória",
    description = "As categorias de `categories.list`, na ordem gravada (a primeira é a padrão), com `key`, `label` e `priority`. Não exige token admin: qualquer token com `read`.",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Categorias", body = CategoryList),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_categories(
    State(state): State<AppState>,
    Auth(principal): Auth,
) -> Result<Json<CategoryList>, ApiError> {
    state.core.list_categories(&principal).await.map(Json)
}

#[utoipa::path(
    post,
    path = "/v1/categories",
    operation_id = "create_category",
    tag = "Settings",
    summary = "Cria uma categoria de memória sem sobrescrever as existentes",
    description = "Acrescenta `{ key, label, priority }` ao fim de `categories.list` (auditoria `category.create`, evento `settings.updated`); a `version` do contexto muda. Repetir com os mesmos campos devolve a existente com `200` e `created: false`; a mesma `key` com outro `label` ou `priority` dá `409 CATEGORY_CONFLICT`; lista cheia, `409 CATEGORY_LIMIT_REACHED`. Editar e remover continuam com `update_settings` (admin). Aceita `Idempotency-Key`. Qualquer token com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = CreateCategoryBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 201, description = "Categoria criada", body = CategoryWrite),
        (status = 200, description = "Categoria equivalente já existia (`created: false`)", body = CategoryWrite),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 409, description = "`CATEGORY_CONFLICT`, `CATEGORY_LIMIT_REACHED` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn create_category(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<CreateCategoryBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .create_category(&principal, body, key.as_deref())
        .await?;
    let status = if result.value.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok(mutation(status, result))
}
