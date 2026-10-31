//! `whoami`, apps, agentes e tokens.

use axum::{Json, extract::State, http::StatusCode, response::Response};

use super::{IdempotencyKey, mutation};
use crate::{
    AppState,
    auth::Auth,
    core::{
        identities::{
            AgentList, AppList, CreateAgentBody, CreateAppBody, IssueTokenBody, IssuedToken,
            ListAgentsQuery, ListQuery, ListTokensQuery, TokenList, UpdateAppBody, UpdateBody,
            UpdateTokenBody, WhoAmI,
        },
        model::{Agent, App, Token},
    },
    error::{ApiError, ErrorResponse},
    http::{ApiJson, ApiPath, ApiQuery},
};

#[utoipa::path(
    get,
    path = "/v1/whoami",
    operation_id = "whoami",
    tag = "Authentication",
    summary = "Identifica o token e lista as operações permitidas",
    description = "Devolve o token, o tipo, os escopos, o app, o agente e as operações que este token pode chamar. Aceito por qualquer token.",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Token autenticado", body = WhoAmI),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn whoami(
    State(state): State<AppState>,
    Auth(principal): Auth,
) -> Result<Json<WhoAmI>, ApiError> {
    state.core.whoami(&principal).await.map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/apps",
    operation_id = "list_apps",
    tag = "Identities",
    summary = "Lista os apps",
    description = "Paginado por `cursor` e `limit`, em ordem de criação. Exige token admin com `read`.",
    params(ListQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Página de apps", body = AppList),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_apps(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> Result<Json<AppList>, ApiError> {
    state.core.list_apps(&principal, query).await.map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/apps/{app}",
    operation_id = "get_app",
    tag = "Identities",
    summary = "Mostra um app",
    description = "`{app}` aceita o id (`app_…`) ou o slug. Exige token admin com `read`.",
    params(("app" = String, Path, description = "Id (`app_…`) ou slug do app")),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "App", body = App),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`APP_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_app(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiPath(app): ApiPath<String>,
) -> Result<Json<App>, ApiError> {
    state.core.get_app(&principal, &app).await.map(Json)
}

#[utoipa::path(
    post,
    path = "/v1/apps",
    operation_id = "create_app",
    tag = "Identities",
    summary = "Cria um app",
    description = "O slug é único na rede. `webhook_url` (http/https, sem credenciais) exige `webhook_secret` (32 a 256 caracteres, só gravado e cifrado). Aceita `Idempotency-Key`. Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = CreateAppBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 201, description = "App criado", body = App),
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
pub async fn create_app(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<CreateAppBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .create_app(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::CREATED, result))
}

#[utoipa::path(
    patch,
    path = "/v1/apps/{app}",
    operation_id = "update_app",
    tag = "Identities",
    summary = "Altera ou desativa um app",
    description = "Campos omitidos não mudam; `null` limpa `description`, `webhook_url` e `webhook_secret`. `disabled: true` desativa o app: os tokens dele deixam de autenticar e ele não recebe eventos. `webhook_url` (http/https, sem credenciais) exige um `webhook_secret` (32 a 256 caracteres, só gravado e cifrado; a leitura mostra `webhook.secret.configured`). Recusado com `LAST_ADMIN_TOKEN` se deixaria a rede sem admin com `write`. Exige token admin com `write`.",
    params(
        ("app" = String, Path, description = "Id (`app_…`) ou slug do app"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")
    ),
    request_body = UpdateAppBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "App alterado", body = App),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`APP_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`LAST_ADMIN_TOKEN` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn update_app(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(app): ApiPath<String>,
    ApiJson(body): ApiJson<UpdateAppBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .update_app(&principal, &app, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    delete,
    path = "/v1/apps/{app}",
    operation_id = "delete_app",
    tag = "Identities",
    summary = "Exclui um app com seus agentes e tokens",
    description = "Remove de vez o app, os agentes e os tokens dele; a auditoria guarda o registro anterior. Para tirar de uso sem perder o vínculo, use `update_app` com `disabled: true`. Exige token admin com `delete`.",
    params(
        ("app" = String, Path, description = "Id (`app_…`) ou slug do app"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "App excluído (estado anterior)", body = App),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`APP_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`LAST_ADMIN_TOKEN` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn delete_app(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(app): ApiPath<String>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .delete_app(&principal, &app, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    get,
    path = "/v1/agents",
    operation_id = "list_agents",
    tag = "Identities",
    summary = "Lista os agentes",
    description = "Filtra por `app` (id ou slug). Paginado por `cursor` e `limit`. Exige token admin com `read`.",
    params(ListAgentsQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Página de agentes", body = AgentList),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`APP_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_agents(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<ListAgentsQuery>,
) -> Result<Json<AgentList>, ApiError> {
    state.core.list_agents(&principal, query).await.map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/agents/{agent}",
    operation_id = "get_agent",
    tag = "Identities",
    summary = "Mostra um agente",
    description = "`{agent}` aceita o id (`ag_…`) ou `app/agente` (codificado como `app%2Fagente`). Exige token admin com `read`.",
    params(("agent" = String, Path, description = "Id (`ag_…`) ou `app/agente`")),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Agente", body = Agent),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`AGENT_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn get_agent(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiPath(agent): ApiPath<String>,
) -> Result<Json<Agent>, ApiError> {
    state.core.get_agent(&principal, &agent).await.map(Json)
}

#[utoipa::path(
    post,
    path = "/v1/agents",
    operation_id = "create_agent",
    tag = "Identities",
    summary = "Cria um agente num app",
    description = "O slug é único dentro do app. Aceita `Idempotency-Key`. Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")),
    request_body = CreateAgentBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 201, description = "Agente criado", body = Agent),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`APP_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`SLUG_ALREADY_EXISTS` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn create_agent(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<CreateAgentBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .create_agent(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::CREATED, result))
}

#[utoipa::path(
    patch,
    path = "/v1/agents/{agent}",
    operation_id = "update_agent",
    tag = "Identities",
    summary = "Altera ou desativa um agente",
    description = "Campos omitidos não mudam. `disabled: true` desativa o agente e os tokens dele deixam de autenticar. Recusado com `LAST_ADMIN_TOKEN` se deixaria a rede sem admin com `write`. Exige token admin com `write`.",
    params(
        ("agent" = String, Path, description = "Id (`ag_…`) ou `app/agente`"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")
    ),
    request_body = UpdateBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Agente alterado", body = Agent),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`AGENT_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`LAST_ADMIN_TOKEN` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn update_agent(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(agent): ApiPath<String>,
    ApiJson(body): ApiJson<UpdateBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .update_agent(&principal, &agent, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    delete,
    path = "/v1/agents/{agent}",
    operation_id = "delete_agent",
    tag = "Identities",
    summary = "Exclui um agente com seus tokens",
    description = "Remove de vez o agente e os tokens dele; a auditoria guarda o registro anterior. Exige token admin com `delete`.",
    params(
        ("agent" = String, Path, description = "Id (`ag_…`) ou `app/agente`"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Agente excluído (estado anterior)", body = Agent),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`AGENT_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`LAST_ADMIN_TOKEN` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn delete_agent(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(agent): ApiPath<String>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .delete_agent(&principal, &agent, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    get,
    path = "/v1/tokens",
    operation_id = "list_tokens",
    tag = "Identities",
    summary = "Lista os tokens, sem segredos",
    description = "Filtra por `app` e `agent`. Mostra o prefixo público, escopos, expiração, último uso e estado; nunca o segredo. Exige token admin com `read`.",
    params(ListTokensQuery),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Página de tokens", body = TokenList),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`APP_NOT_FOUND` ou `AGENT_NOT_FOUND`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn list_tokens(
    State(state): State<AppState>,
    Auth(principal): Auth,
    ApiQuery(query): ApiQuery<ListTokensQuery>,
) -> Result<Json<TokenList>, ApiError> {
    state.core.list_tokens(&principal, query).await.map(Json)
}

#[utoipa::path(
    post,
    path = "/v1/tokens",
    operation_id = "issue_token",
    tag = "Identities",
    summary = "Emite um token; o segredo aparece só nesta resposta",
    description = "Token `agent` exige `agent`; token `admin` aceita um agente do mesmo app. `app` pode ser omitido quando `agent` é informado. Os escopos nunca vão além dos do emissor (`SCOPE_REQUIRED`). Com `Idempotency-Key`, a repetição devolve o token sem `secret`. Exige token admin com `write`.",
    params(("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada (sem o segredo) por 24 h")),
    request_body = IssueTokenBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 201, description = "Token emitido", body = IssuedToken),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`APP_NOT_FOUND` ou `AGENT_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn issue_token(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<IssueTokenBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .issue_token(&principal, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::CREATED, result))
}

#[utoipa::path(
    patch,
    path = "/v1/tokens/{token}",
    operation_id = "update_token",
    tag = "Identities",
    summary = "Altera rótulo, escopos ou expiração de um token",
    description = "Campos omitidos não mudam; `label` e `expires_at` aceitam `null`. Os escopos nunca vão além dos do emissor. Recusado com `LAST_ADMIN_TOKEN` se deixaria a rede sem admin com `write`. Exige token admin com `write`.",
    params(
        ("token" = String, Path, description = "Id do token (`tk_…`)"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada para o mesmo corpo por 24 h")
    ),
    request_body = UpdateTokenBody,
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Token alterado", body = Token),
        (status = 400, description = "`VALIDATION_ERROR`", body = ErrorResponse),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`TOKEN_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`LAST_ADMIN_TOKEN` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 413, description = "`PAYLOAD_TOO_LARGE`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn update_token(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(token): ApiPath<String>,
    ApiJson(body): ApiJson<UpdateTokenBody>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .update_token(&principal, &token, body, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}

#[utoipa::path(
    post,
    path = "/v1/tokens/{token}/revoke",
    operation_id = "revoke_token",
    tag = "Identities",
    summary = "Revoga um token imediatamente",
    description = "O token deixa de autenticar na próxima requisição. Revogar de novo não muda nada. Recusado com `LAST_ADMIN_TOKEN` para o último admin ativo com `write`. Exige token admin com `delete`.",
    params(
        ("token" = String, Path, description = "Id do token (`tk_…`)"),
        ("Idempotency-Key" = Option<String>, Header, description = "Repete a resposta gravada por 24 h")
    ),
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Token revogado", body = Token),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 404, description = "`TOKEN_NOT_FOUND`", body = ErrorResponse),
        (status = 409, description = "`LAST_ADMIN_TOKEN` ou `IDEMPOTENCY_KEY_CONFLICT`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn revoke_token(
    State(state): State<AppState>,
    Auth(principal): Auth,
    key: IdempotencyKey,
    ApiPath(token): ApiPath<String>,
) -> Result<Response, ApiError> {
    let result = state
        .core
        .revoke_token(&principal, &token, key.as_deref())
        .await?;
    Ok(mutation(StatusCode::OK, result))
}
