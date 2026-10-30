//! Apps, agentes e tokens: validação, regras de escopo e as operações administrativas.

use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use super::{
    Core, authz,
    idempotency::Mutation,
    ids,
    model::{
        Agent, AgentLookup, App, AppPatch, Lookup, NewAgent, NewApp, NewToken, Token, TokenFilter,
        TokenPatch, double_option,
    },
    pagination::{Page, PageRequest},
    principal::{AgentRef, AppRef, Principal, Scope, ScopeSet, TokenKind},
    store_error,
};
use crate::{auth::token::TokenService, error::ApiError, store::Store};

const MAX_SLUG: usize = 64;
pub(crate) const MAX_NAME: usize = 120;
pub(crate) const MAX_DESCRIPTION: usize = 1000;
const MAX_LABEL: usize = 120;

// ---------- entradas ----------

/// Paginação comum das listas.
#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// Cursor opaco devolvido em `next_cursor`.
    pub cursor: Option<String>,
    /// Itens por página, de 1 a 200 (padrão 50).
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct ListAgentsQuery {
    /// Só os agentes deste app (id ou slug).
    pub app: Option<String>,
    pub cursor: Option<String>,
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct ListTokensQuery {
    /// Só os tokens deste app (id ou slug).
    pub app: Option<String>,
    /// Só os tokens deste agente (id ou `app/agente`).
    pub agent: Option<String>,
    pub cursor: Option<String>,
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAppBody {
    /// Minúsculas, dígitos e hífens (1 a 64), único na rede.
    #[schema(example = "app-a")]
    pub slug: String,
    #[schema(example = "App A")]
    pub name: String,
    pub description: Option<String>,
}

/// Campos omitidos não mudam; `description: null` limpa; `disabled` desativa ou reativa.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateBody {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    /// `true` desativa (os tokens deixam de autenticar); `false` reativa.
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAgentBody {
    /// App dono (id ou slug).
    #[schema(example = "app-a")]
    pub app: String,
    /// Único dentro do app.
    #[schema(example = "curador")]
    pub slug: String,
    #[schema(example = "Curador")]
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueTokenBody {
    /// App dono (id ou slug); pode ser omitido quando `agent` é informado.
    #[schema(example = "app-a")]
    pub app: Option<String>,
    /// Agente (id ou `app/agente`); obrigatório para `kind: agent`.
    #[schema(example = "app-a/curador")]
    pub agent: Option<String>,
    pub kind: TokenKind,
    /// `read`, `read+write`, `read+delete` ou `read+write+delete`; nunca além dos do emissor.
    #[schema(example = json!(["read", "write"]))]
    pub scopes: Vec<Scope>,
    #[schema(example = "curador producao")]
    pub label: Option<String>,
    /// RFC 3339 no futuro; `null` não expira.
    pub expires_at: Option<String>,
}

/// Campos omitidos não mudam; `label` e `expires_at` aceitam `null`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTokenBody {
    #[serde(default, deserialize_with = "double_option")]
    pub label: Option<Option<String>>,
    pub scopes: Option<Vec<Scope>>,
    #[serde(default, deserialize_with = "double_option")]
    pub expires_at: Option<Option<String>>,
}

// ---------- saídas ----------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WhoAmI {
    #[schema(example = "tk_3f9e1a7c")]
    pub token_id: String,
    pub kind: TokenKind,
    pub scopes: Vec<Scope>,
    pub app: AppRef,
    pub agent: Option<AgentRef>,
    /// Operações que este token pode chamar.
    pub operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AppList {
    pub apps: Vec<App>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentList {
    pub agents: Vec<Agent>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TokenList {
    pub tokens: Vec<Token>,
    pub next_cursor: Option<String>,
}

/// Token emitido. `secret` só existe na primeira resposta; a repetição idempotente o omite.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssuedToken {
    pub token: Token,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "eng.tk_3f9e1a7c.<segredo>")]
    pub secret: Option<String>,
}

pub(crate) fn next_cursor<T>(page: &Page<T>) -> Option<String> {
    page.next.as_ref().map(super::pagination::Cursor::encode)
}

// ---------- validação ----------

pub fn validate_slug(slug: &str) -> Result<(), ApiError> {
    let valid = (1..=MAX_SLUG).contains(&slug.len())
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--");
    if valid {
        Ok(())
    } else {
        Err(ApiError::validation(
            "slug deve ter de 1 a 64 caracteres: minusculas, digitos e hifens simples",
        ))
    }
}

pub(crate) fn text(field: &str, value: &str, max: usize) -> Result<String, ApiError> {
    let value = value.trim();
    let length = value.chars().count();
    if (1..=max).contains(&length) {
        Ok(value.to_owned())
    } else {
        Err(ApiError::validation(format!(
            "{field} deve ter de 1 a {max} caracteres"
        )))
    }
}

/// Texto opcional: vazio vira `None`.
pub(crate) fn optional_text(
    field: &str,
    value: Option<&str>,
    max: usize,
) -> Result<Option<String>, ApiError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => text(field, value, max).map(Some),
        None => Ok(None),
    }
}

pub(crate) fn scopes(values: &[Scope]) -> Result<ScopeSet, ApiError> {
    ScopeSet::from_scopes(values).map_err(|error| ApiError::validation(error.to_string()))
}

/// RFC 3339 no futuro, normalizado para o formato persistido.
fn expiration(value: &str) -> Result<String, ApiError> {
    let parsed = ids::parse_timestamp(value.trim())
        .ok_or_else(|| ApiError::validation("expires_at deve ser uma data RFC 3339"))?;
    if parsed <= Utc::now() {
        return Err(ApiError::validation("expires_at deve estar no futuro"));
    }
    Ok(ids::timestamp(parsed))
}

/// O emissor não concede escopos que não tem (FR-005).
fn ensure_grantable(principal: &Principal, granted: ScopeSet) -> Result<(), ApiError> {
    if granted.is_subset_of(principal.scopes) {
        return Ok(());
    }
    let missing = granted
        .scopes()
        .into_iter()
        .find(|scope| !principal.scopes.contains(*scope))
        .unwrap_or(Scope::Read);
    Err(ApiError::new(
        "SCOPE_REQUIRED",
        format!(
            "Nao e possivel conceder o escopo {} sem possui-lo",
            missing.as_str()
        ),
    )
    .with_details(json!({ "required_kind": "admin", "required_scope": missing.as_str() })))
}

fn app_patch(body: &UpdateBody) -> Result<AppPatch, ApiError> {
    if body.name.is_none() && body.description.is_none() && body.disabled.is_none() {
        return Err(ApiError::validation(
            "Informe ao menos um campo para alterar",
        ));
    }
    Ok(AppPatch {
        name: body
            .name
            .as_deref()
            .map(|name| text("name", name, MAX_NAME))
            .transpose()?,
        description: body
            .description
            .as_ref()
            .map(|description| {
                optional_text("description", description.as_deref(), MAX_DESCRIPTION)
            })
            .transpose()?,
        disabled: body.disabled,
    })
}

pub fn app_not_found() -> ApiError {
    ApiError::not_found("APP_NOT_FOUND", "App nao encontrado")
}

pub fn agent_not_found() -> ApiError {
    ApiError::not_found("AGENT_NOT_FOUND", "Agente nao encontrado")
}

pub fn token_not_found() -> ApiError {
    ApiError::not_found("TOKEN_NOT_FOUND", "Token nao encontrado")
}

/// `app_…` é id; o resto é slug.
pub fn app_lookup(value: &str) -> Lookup {
    let value = value.trim();
    if ids::is_valid(ids::APP_PREFIX, value) {
        Lookup::Id(value.to_owned())
    } else {
        Lookup::Slug(value.to_owned())
    }
}

/// `ag_…` é id; `app/agente` é slug qualificado.
pub fn agent_lookup(value: &str) -> Result<AgentLookup, ApiError> {
    let value = value.trim();
    if ids::is_valid(ids::AGENT_PREFIX, value) {
        return Ok(AgentLookup::Id(value.to_owned()));
    }
    match value.split_once('/') {
        Some((app, agent)) if !app.is_empty() && !agent.is_empty() && !agent.contains('/') => {
            Ok(AgentLookup::Slug {
                app: app.to_owned(),
                agent: agent.to_owned(),
            })
        }
        _ => Err(ApiError::validation(
            "agente deve ser um id (ag_...) ou app/agente",
        )),
    }
}

fn token_id(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if ids::is_valid(ids::TOKEN_PREFIX, value) {
        Ok(value.to_owned())
    } else {
        Err(token_not_found())
    }
}

// ---------- operações ----------

impl<S: Store> Core<S> {
    pub async fn whoami(&self, principal: &Principal) -> Result<WhoAmI, ApiError> {
        self.authorize(principal, "whoami")?;
        Ok(WhoAmI {
            token_id: principal.token_id.clone(),
            kind: principal.kind,
            scopes: principal.scopes.scopes(),
            app: principal.app.clone(),
            agent: principal.agent.clone(),
            operations: authz::allowed_operations(principal)
                .into_iter()
                .map(str::to_owned)
                .collect(),
        })
    }

    async fn find_app(&self, value: &str) -> Result<App, ApiError> {
        self.store()
            .get_app(app_lookup(value))
            .await?
            .ok_or_else(app_not_found)
    }

    pub(crate) async fn find_agent(&self, value: &str) -> Result<Agent, ApiError> {
        self.store()
            .get_agent(agent_lookup(value)?)
            .await?
            .ok_or_else(agent_not_found)
    }

    pub async fn list_apps(
        &self,
        principal: &Principal,
        query: ListQuery,
    ) -> Result<AppList, ApiError> {
        self.authorize(principal, "list_apps")?;
        let page = PageRequest::new(query.cursor.as_deref(), query.limit)?;
        let page = self.store().list_apps(page).await?;
        Ok(AppList {
            next_cursor: next_cursor(&page),
            apps: page.items,
        })
    }

    pub async fn get_app(&self, principal: &Principal, app: &str) -> Result<App, ApiError> {
        self.authorize(principal, "get_app")?;
        self.find_app(app).await
    }

    pub async fn create_app(
        &self,
        principal: &Principal,
        body: CreateAppBody,
        key: Option<&str>,
    ) -> Result<Mutation<App>, ApiError> {
        let operation = self.authorize(principal, "create_app")?;
        let slug = body.slug.trim().to_owned();
        validate_slug(&slug)?;
        let new = NewApp {
            slug,
            name: text("name", &body.name, MAX_NAME)?,
            description: optional_text(
                "description",
                body.description.as_deref(),
                MAX_DESCRIPTION,
            )?,
        };
        let core = self.clone();
        self.mutate(
            principal,
            operation,
            key,
            &body,
            201,
            |context| async move {
                core.store()
                    .create_app(new, context)
                    .await
                    .map_err(|error| store_error(error, app_not_found))
            },
        )
        .await
    }

    pub async fn update_app(
        &self,
        principal: &Principal,
        app: &str,
        body: UpdateBody,
        key: Option<&str>,
    ) -> Result<Mutation<App>, ApiError> {
        let operation = self.authorize(principal, "update_app")?;
        let patch = app_patch(&body)?;
        let input = json!({ "app": app, "body": body });
        let core = self.clone();
        let app = app.to_owned();
        self.mutate(
            principal,
            operation,
            key,
            &input,
            200,
            |context| async move {
                let id = core.find_app(&app).await?.id;
                core.store()
                    .update_app(id, patch, context)
                    .await
                    .map_err(|error| store_error(error, app_not_found))
            },
        )
        .await
    }

    pub async fn delete_app(
        &self,
        principal: &Principal,
        app: &str,
        key: Option<&str>,
    ) -> Result<Mutation<App>, ApiError> {
        let operation = self.authorize(principal, "delete_app")?;
        let input = json!({ "app": app });
        let core = self.clone();
        let app = app.to_owned();
        self.mutate(
            principal,
            operation,
            key,
            &input,
            200,
            |context| async move {
                let id = core.find_app(&app).await?.id;
                core.store()
                    .delete_app(id, context)
                    .await
                    .map_err(|error| store_error(error, app_not_found))
            },
        )
        .await
        .inspect(|_| self.network_changed())
    }

    pub async fn list_agents(
        &self,
        principal: &Principal,
        query: ListAgentsQuery,
    ) -> Result<AgentList, ApiError> {
        self.authorize(principal, "list_agents")?;
        let page = PageRequest::new(query.cursor.as_deref(), query.limit)?;
        let app_id = match query
            .app
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            Some(app) => Some(self.find_app(app).await?.id),
            None => None,
        };
        let page = self.store().list_agents(app_id, page).await?;
        Ok(AgentList {
            next_cursor: next_cursor(&page),
            agents: page.items,
        })
    }

    pub async fn get_agent(&self, principal: &Principal, agent: &str) -> Result<Agent, ApiError> {
        self.authorize(principal, "get_agent")?;
        self.find_agent(agent).await
    }

    pub async fn create_agent(
        &self,
        principal: &Principal,
        body: CreateAgentBody,
        key: Option<&str>,
    ) -> Result<Mutation<Agent>, ApiError> {
        let operation = self.authorize(principal, "create_agent")?;
        let slug = body.slug.trim().to_owned();
        validate_slug(&slug)?;
        let name = text("name", &body.name, MAX_NAME)?;
        let description =
            optional_text("description", body.description.as_deref(), MAX_DESCRIPTION)?;
        let core = self.clone();
        let app = body.app.clone();
        self.mutate(
            principal,
            operation,
            key,
            &body,
            201,
            |context| async move {
                let app = core.find_app(&app).await?;
                let new = NewAgent {
                    app_id: app.id,
                    slug,
                    name,
                    description,
                };
                core.store()
                    .create_agent(new, context)
                    .await
                    .map_err(|error| store_error(error, app_not_found))
            },
        )
        .await
    }

    pub async fn update_agent(
        &self,
        principal: &Principal,
        agent: &str,
        body: UpdateBody,
        key: Option<&str>,
    ) -> Result<Mutation<Agent>, ApiError> {
        let operation = self.authorize(principal, "update_agent")?;
        let patch = app_patch(&body)?;
        agent_lookup(agent)?;
        let input = json!({ "agent": agent, "body": body });
        let core = self.clone();
        let agent = agent.to_owned();
        self.mutate(
            principal,
            operation,
            key,
            &input,
            200,
            |context| async move {
                let id = core.find_agent(&agent).await?.id;
                core.store()
                    .update_agent(id, patch, context)
                    .await
                    .map_err(|error| store_error(error, agent_not_found))
            },
        )
        .await
    }

    pub async fn delete_agent(
        &self,
        principal: &Principal,
        agent: &str,
        key: Option<&str>,
    ) -> Result<Mutation<Agent>, ApiError> {
        let operation = self.authorize(principal, "delete_agent")?;
        agent_lookup(agent)?;
        let input = json!({ "agent": agent });
        let core = self.clone();
        let agent = agent.to_owned();
        self.mutate(
            principal,
            operation,
            key,
            &input,
            200,
            |context| async move {
                let id = core.find_agent(&agent).await?.id;
                core.store()
                    .delete_agent(id, context)
                    .await
                    .map_err(|error| store_error(error, agent_not_found))
            },
        )
        .await
        .inspect(|_| self.network_changed())
    }

    pub async fn list_tokens(
        &self,
        principal: &Principal,
        query: ListTokensQuery,
    ) -> Result<TokenList, ApiError> {
        self.authorize(principal, "list_tokens")?;
        let page = PageRequest::new(query.cursor.as_deref(), query.limit)?;
        let mut filter = TokenFilter::default();
        if let Some(app) = query
            .app
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            filter.app_id = Some(self.find_app(app).await?.id);
        }
        if let Some(agent) = query
            .agent
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            filter.agent_id = Some(self.find_agent(agent).await?.id);
        }
        let page = self.store().list_tokens(filter, page).await?;
        Ok(TokenList {
            next_cursor: next_cursor(&page),
            tokens: page.items,
        })
    }

    pub async fn issue_token(
        &self,
        principal: &Principal,
        body: IssueTokenBody,
        key: Option<&str>,
    ) -> Result<Mutation<IssuedToken>, ApiError> {
        let operation = self.authorize(principal, "issue_token")?;
        let granted = scopes(&body.scopes)?;
        ensure_grantable(principal, granted)?;
        let label = optional_text("label", body.label.as_deref(), MAX_LABEL)?;
        let expires_at = body.expires_at.as_deref().map(expiration).transpose()?;
        let agent_value = body.agent.clone().filter(|value| !value.trim().is_empty());
        if body.kind == TokenKind::Agent && agent_value.is_none() {
            return Err(ApiError::validation("token agent exige agent"));
        }
        if let Some(agent) = &agent_value {
            agent_lookup(agent)?;
        }
        let app_value = body.app.clone().filter(|value| !value.trim().is_empty());
        if app_value.is_none() && agent_value.is_none() {
            return Err(ApiError::validation("informe app ou agent"));
        }
        let generated = self.tokens().generate();
        let digest = generated.digest.clone();
        let core = self.clone();
        let kind = body.kind;
        let mutation = self
            .mutate(
                principal,
                operation,
                key,
                &body,
                201,
                |context| async move {
                    let agent = match agent_value {
                        Some(agent) => Some(core.find_agent(&agent).await?),
                        None => None,
                    };
                    let app_id = match (app_value, &agent) {
                        (Some(app), agent) => {
                            let app = core.find_app(&app).await?;
                            if agent.as_ref().is_some_and(|agent| agent.app.id != app.id) {
                                return Err(ApiError::validation("o agente nao pertence ao app"));
                            }
                            app.id
                        }
                        (None, Some(agent)) => agent.app.id.clone(),
                        (None, None) => return Err(ApiError::validation("informe app ou agent")),
                    };
                    let new = NewToken {
                        id: None,
                        app_id,
                        agent_id: agent.map(|agent| agent.id),
                        kind,
                        scopes: granted.scopes(),
                        label,
                        expires_at,
                        digest,
                    };
                    core.store()
                        .create_token(new, context)
                        .await
                        .map_err(|error| store_error(error, app_not_found))
                },
            )
            .await?;
        let secret = (!mutation.replayed).then(|| {
            TokenService::plaintext(&mutation.value.id, &generated.secret)
                .expose()
                .to_owned()
        });
        Ok(Mutation {
            value: IssuedToken {
                token: mutation.value,
                secret,
            },
            replayed: mutation.replayed,
        })
    }

    pub async fn update_token(
        &self,
        principal: &Principal,
        token: &str,
        body: UpdateTokenBody,
        key: Option<&str>,
    ) -> Result<Mutation<Token>, ApiError> {
        let operation = self.authorize(principal, "update_token")?;
        if body.label.is_none() && body.scopes.is_none() && body.expires_at.is_none() {
            return Err(ApiError::validation(
                "Informe ao menos um campo para alterar",
            ));
        }
        let id = token_id(token)?;
        let granted = body.scopes.as_deref().map(scopes).transpose()?;
        if let Some(granted) = granted {
            ensure_grantable(principal, granted)?;
        }
        let patch = TokenPatch {
            label: body
                .label
                .as_ref()
                .map(|label| optional_text("label", label.as_deref(), MAX_LABEL))
                .transpose()?,
            scopes: granted.map(ScopeSet::scopes),
            expires_at: body
                .expires_at
                .as_ref()
                .map(|value| value.as_deref().map(expiration).transpose())
                .transpose()?,
        };
        let input = json!({ "token": id, "body": body });
        let core = self.clone();
        self.mutate(
            principal,
            operation,
            key,
            &input,
            200,
            |context| async move {
                core.store()
                    .update_token(id, patch, context)
                    .await
                    .map_err(|error| store_error(error, token_not_found))
            },
        )
        .await
    }

    pub async fn revoke_token(
        &self,
        principal: &Principal,
        token: &str,
        key: Option<&str>,
    ) -> Result<Mutation<Token>, ApiError> {
        let operation = self.authorize(principal, "revoke_token")?;
        let id = token_id(token)?;
        let input = json!({ "token": id });
        let core = self.clone();
        self.mutate(
            principal,
            operation,
            key,
            &input,
            200,
            |context| async move {
                core.store()
                    .revoke_token(id, context)
                    .await
                    .map_err(|error| store_error(error, token_not_found))
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_lowercase_kebab_case() {
        for valid in ["app-a", "a", "curador-2", "x".repeat(64).as_str()] {
            assert!(validate_slug(valid).is_ok(), "{valid}");
        }
        for invalid in ["", "App", "a_b", "-a", "a-", "a--b", "a/b", &"x".repeat(65)] {
            assert!(validate_slug(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn lookups_distinguish_ids_from_slugs() {
        assert_eq!(
            app_lookup("app_3f9e1a7c"),
            Lookup::Id("app_3f9e1a7c".to_owned())
        );
        assert_eq!(app_lookup("app-a"), Lookup::Slug("app-a".to_owned()));
        assert_eq!(
            agent_lookup("app-a/curador").expect("slug"),
            AgentLookup::Slug {
                app: "app-a".to_owned(),
                agent: "curador".to_owned()
            }
        );
        assert_eq!(
            agent_lookup("ag_5b1c9d2e").expect("id"),
            AgentLookup::Id("ag_5b1c9d2e".to_owned())
        );
        assert!(agent_lookup("curador").is_err());
        assert!(agent_lookup("a/b/c").is_err());
    }

    #[test]
    fn expiration_must_be_a_future_rfc3339_date() {
        assert!(expiration("2999-01-01T00:00:00Z").is_ok());
        assert!(expiration("2000-01-01T00:00:00Z").is_err());
        assert!(expiration("amanha").is_err());
    }
}
