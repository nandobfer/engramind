//! Ferramentas MCP: uma por operação do catálogo, com os mesmos parâmetros, escopos e códigos
//! de erro do REST. `tools/list` mostra só as permitidas ao token (FR-011) e os hints vêm de
//! `OPERATIONS` (FR-012).

use std::{collections::BTreeMap, time::Instant};

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
        ToolAnnotations,
    },
    service::{RequestContext, RoleServer},
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    core::{
        Core,
        audit::AuditQuery,
        authz,
        channels::{CreateChannelBody, ListChannelsQuery, UpdateChannelBody},
        docs::{GetGuideQuery, ListOperationsQuery, SearchDocsQuery},
        identities::{
            CreateAgentBody, CreateAppBody, IssueTokenBody, ListAgentsQuery, ListQuery,
            ListTokensQuery, UpdateBody, UpdateTokenBody,
        },
        operations::{self, OPERATIONS},
        principal::Principal,
        settings::{GetSettingsQuery, UpdateSettingsBody},
        subscriptions::{ListSubscriptionsQuery, SubscribeBody, UpdateSubscriptionBody},
    },
    error::ApiError,
    rest::ChannelsHeader,
    store::AppStore,
};

pub const SERVER_NAME: &str = "engramind";

const INSTRUCTIONS: &str = "Engramind: rede de memorias para agentes de IA. As ferramentas sao as \
operacoes da API, com os mesmos nomes, parametros, escopos e erros do REST; so aparecem as que este \
token pode usar. O cabecalho Engramind-Channels da conexao restringe os canais. Comece por \
whoami e get_overview; use get_guide e search_docs para as regras e \
get_operation/get_schema para o contrato exato. Erros de dominio voltam com isError e o envelope \
{ error: { code, message, details } }. Mutacoes aceitam idempotency_key.";

#[derive(Clone)]
pub struct EngramindServer {
    core: Core<AppStore>,
    tool_router: ToolRouter<Self>,
}

/// Argumento comum das mutações.
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct Idempotency {
    /// Mesmo efeito do cabeçalho `Idempotency-Key` do REST.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppRequest {
    /// Id (`app_…`) ou slug do app.
    pub app: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppMutationRequest {
    /// Id (`app_…`) ou slug do app.
    pub app: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateAppRequest {
    #[serde(flatten)]
    pub body: CreateAppBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateAppRequest {
    /// Id (`app_…`) ou slug do app.
    pub app: String,
    #[serde(flatten)]
    pub body: UpdateBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentRequest {
    /// Id (`ag_…`) ou `app/agente`.
    pub agent: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentMutationRequest {
    /// Id (`ag_…`) ou `app/agente`.
    pub agent: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateAgentRequest {
    #[serde(flatten)]
    pub body: CreateAgentBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateAgentRequest {
    /// Id (`ag_…`) ou `app/agente`.
    pub agent: String,
    #[serde(flatten)]
    pub body: UpdateBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IssueTokenRequest {
    #[serde(flatten)]
    pub body: IssueTokenBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateTokenRequest {
    /// Id do token (`tk_…`).
    pub token: String,
    #[serde(flatten)]
    pub body: UpdateTokenBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TokenMutationRequest {
    /// Id do token (`tk_…`).
    pub token: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelRequest {
    /// Id (`ch_…`) ou slug do canal (`eng/global`).
    pub channel: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelMutationRequest {
    /// Id (`ch_…`) ou slug do canal (`eng/global`).
    pub channel: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateChannelRequest {
    #[serde(flatten)]
    pub body: CreateChannelBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateChannelRequest {
    /// Id (`ch_…`) ou slug do canal (`eng/global`).
    pub channel: String,
    #[serde(flatten)]
    pub body: UpdateChannelBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubscribeRequest {
    #[serde(flatten)]
    pub body: SubscribeBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateSubscriptionRequest {
    /// Id da assinatura (`sub_…`).
    pub subscription: String,
    #[serde(flatten)]
    pub body: UpdateSubscriptionBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionMutationRequest {
    /// Id da assinatura (`sub_…`).
    pub subscription: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettingsRequest {
    /// Mapa parcial `chave → valor`; `null` volta ao padrão.
    pub settings: BTreeMap<String, Value>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuideRequest {
    /// Id do guia devolvido por `list_guides`, por exemplo `tokens-e-escopos`.
    pub guide: String,
    /// Título de uma seção; acentos e caixa são ignorados.
    #[serde(default)]
    pub section: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationRequest {
    /// `operationId`, igual ao nome base, por exemplo `issue_token`.
    pub operation: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchemaRequest {
    /// Nome em `components.schemas`, por exemplo `WhoAmI`.
    pub schema: String,
}

/// O `Principal` resolvido pelo middleware Bearer.
fn principal(context: &RequestContext<RoleServer>) -> Result<Principal, ErrorData> {
    context
        .extensions
        .get::<axum::http::request::Parts>()
        .and_then(|parts| parts.extensions.get::<Principal>())
        .cloned()
        // A rota é protegida por `require_auth`; identidade ausente é erro de montagem.
        .ok_or_else(|| ErrorData::internal_error("identidade do token indisponivel", None))
}

/// Cabeçalho `Engramind-Channels` da requisição HTTP que carregou a mensagem MCP: o filtro
/// padrão de canais da conexão (desenho §5).
fn channels_header(context: &RequestContext<RoleServer>) -> Result<Option<String>, ApiError> {
    match context.extensions.get::<axum::http::request::Parts>() {
        Some(parts) => ChannelsHeader::from_headers(&parts.headers).map(|header| header.0),
        None => Ok(None),
    }
}

/// Erro de domínio: `isError: true` com o envelope padrão em `structuredContent` (D-005).
fn domain_error(error: &ApiError) -> CallToolResult {
    CallToolResult::structured_error(error.envelope())
}

fn reply<T: Serialize>(result: Result<T, ApiError>) -> Result<CallToolResult, ErrorData> {
    Ok(match result {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => CallToolResult::structured(value),
            Err(_) => domain_error(&ApiError::internal()),
        },
        Err(error) => domain_error(&error),
    })
}

fn text_reply(result: Result<String, ApiError>) -> Result<CallToolResult, ErrorData> {
    Ok(match result {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(error) => domain_error(&error),
    })
}

/// Argumentos fora do esquema voltam do `rmcp` como texto; viram `VALIDATION_ERROR`, como
/// no REST.
fn validation_envelope(response: CallToolResponse) -> CallToolResponse {
    match response {
        CallToolResponse::Complete(result)
            if result.is_error == Some(true) && result.structured_content.is_none() =>
        {
            let reason = result
                .content
                .iter()
                .filter_map(|block| block.as_text().map(|text| text.text.clone()))
                .collect::<Vec<_>>()
                .join(" ");
            let error = ApiError::validation("Argumentos invalidos para a ferramenta")
                .with_details(serde_json::json!({ "reason": reason }));
            CallToolResponse::Complete(domain_error(&error))
        }
        other => other,
    }
}

/// Anotações e descrição de cada ferramenta a partir do catálogo de operações.
fn annotate(router: &mut ToolRouter<EngramindServer>) {
    for route in router.map.values_mut() {
        if let Some(operation) = operations::find(&route.attr.name) {
            route.attr.description = Some(operation.summary.into());
            route.attr.annotations = Some(
                ToolAnnotations::new()
                    .read_only(operation.read_only)
                    .destructive(operation.destructive)
                    .idempotent(operation.idempotent)
                    .open_world(false),
            );
            route.attr.title = Some(operation.summary.to_owned());
        }
    }
}

#[tool_router]
impl EngramindServer {
    pub fn new(core: Core<AppStore>) -> Self {
        let mut tool_router = Self::tool_router();
        annotate(&mut tool_router);
        Self { core, tool_router }
    }

    /// Todas as ferramentas, sem filtro (para os testes de contrato).
    pub fn all_tools(&self) -> Vec<Tool> {
        self.tool_router.list_all()
    }

    #[tool]
    async fn whoami(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.whoami(&principal).await)
    }

    #[tool]
    async fn list_apps(
        &self,
        Parameters(query): Parameters<ListQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.list_apps(&principal, query).await)
    }

    #[tool]
    async fn get_app(
        &self,
        Parameters(request): Parameters<AppRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.get_app(&principal, &request.app).await)
    }

    #[tool]
    async fn create_app(
        &self,
        Parameters(request): Parameters<CreateAppRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .create_app(&principal, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn update_app(
        &self,
        Parameters(request): Parameters<UpdateAppRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .update_app(&principal, &request.app, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn delete_app(
        &self,
        Parameters(request): Parameters<AppMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .delete_app(&principal, &request.app, request.idempotency_key.as_deref())
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn list_agents(
        &self,
        Parameters(query): Parameters<ListAgentsQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.list_agents(&principal, query).await)
    }

    #[tool]
    async fn get_agent(
        &self,
        Parameters(request): Parameters<AgentRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.get_agent(&principal, &request.agent).await)
    }

    #[tool]
    async fn create_agent(
        &self,
        Parameters(request): Parameters<CreateAgentRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .create_agent(&principal, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn update_agent(
        &self,
        Parameters(request): Parameters<UpdateAgentRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .update_agent(&principal, &request.agent, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn delete_agent(
        &self,
        Parameters(request): Parameters<AgentMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .delete_agent(
                    &principal,
                    &request.agent,
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn list_tokens(
        &self,
        Parameters(query): Parameters<ListTokensQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.list_tokens(&principal, query).await)
    }

    #[tool]
    async fn issue_token(
        &self,
        Parameters(request): Parameters<IssueTokenRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .issue_token(&principal, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn update_token(
        &self,
        Parameters(request): Parameters<UpdateTokenRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .update_token(&principal, &request.token, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn revoke_token(
        &self,
        Parameters(request): Parameters<TokenMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .revoke_token(
                    &principal,
                    &request.token,
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn list_channels(
        &self,
        Parameters(query): Parameters<ListChannelsQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .list_channels(&principal, query, header.as_deref())
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result)
    }

    #[tool]
    async fn get_channel(
        &self,
        Parameters(request): Parameters<ChannelRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .get_channel(&principal, &request.channel, header.as_deref())
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result)
    }

    #[tool]
    async fn create_channel(
        &self,
        Parameters(request): Parameters<CreateChannelRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .create_channel(&principal, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn update_channel(
        &self,
        Parameters(request): Parameters<UpdateChannelRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .update_channel(&principal, &request.channel, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn archive_channel(
        &self,
        Parameters(request): Parameters<ChannelMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .archive_channel(
                    &principal,
                    &request.channel,
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn restore_channel(
        &self,
        Parameters(request): Parameters<ChannelMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .restore_channel(
                    &principal,
                    &request.channel,
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn delete_channel(
        &self,
        Parameters(request): Parameters<ChannelMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .delete_channel(
                    &principal,
                    &request.channel,
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn list_subscriptions(
        &self,
        Parameters(query): Parameters<ListSubscriptionsQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.list_subscriptions(&principal, query).await)
    }

    #[tool]
    async fn subscribe(
        &self,
        Parameters(request): Parameters<SubscribeRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .subscribe(&principal, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn update_subscription(
        &self,
        Parameters(request): Parameters<UpdateSubscriptionRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        reply(
            self.core
                .update_subscription(&principal, &request.subscription, request.body, key)
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn unsubscribe(
        &self,
        Parameters(request): Parameters<SubscriptionMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .unsubscribe(
                    &principal,
                    &request.subscription,
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn get_settings(
        &self,
        Parameters(query): Parameters<GetSettingsQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.get_settings(&principal, query).await)
    }

    #[tool]
    async fn update_settings(
        &self,
        Parameters(request): Parameters<UpdateSettingsRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .update_settings(
                    &principal,
                    UpdateSettingsBody(request.settings),
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn get_status(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.get_status(&principal).await)
    }

    #[tool]
    async fn get_audit(
        &self,
        Parameters(query): Parameters<AuditQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.get_audit(&principal, query).await)
    }

    #[tool]
    async fn get_overview(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        text_reply(self.core.get_overview(&principal).map(|doc| doc.content))
    }

    #[tool]
    async fn list_guides(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.list_guides(&principal))
    }

    #[tool]
    async fn get_guide(
        &self,
        Parameters(request): Parameters<GuideRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let query = GetGuideQuery {
            section: request.section,
        };
        text_reply(
            self.core
                .get_guide(&principal, &request.guide, query)
                .map(|doc| doc.content),
        )
    }

    #[tool]
    async fn search_docs(
        &self,
        Parameters(query): Parameters<SearchDocsQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.search_docs(&principal, query))
    }

    #[tool]
    async fn list_operations(
        &self,
        Parameters(query): Parameters<ListOperationsQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.list_operations(&principal, query))
    }

    #[tool]
    async fn get_operation(
        &self,
        Parameters(request): Parameters<OperationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.get_operation(&principal, &request.operation))
    }

    #[tool]
    async fn get_schema(
        &self,
        Parameters(request): Parameters<SchemaRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.get_schema(&principal, &request.schema))
    }
}

impl ServerHandler for EngramindServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION")).with_title("Engramind"),
            )
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let principal = principal(&context)?;
        let mut tools: Vec<Tool> = self
            .tool_router
            .list_all()
            .into_iter()
            .filter(|tool| {
                operations::find(&tool.name)
                    .is_some_and(|operation| authz::allows(&principal, operation))
            })
            .collect();
        tools.sort_by_key(|tool| {
            OPERATIONS
                .iter()
                .position(|operation| operation.name == tool.name)
                .unwrap_or(usize::MAX)
        });
        Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router.get(name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let started = Instant::now();
        // Só nomes do catálogo vão para o log; os argumentos nunca.
        let operation = operations::find(&request.name);
        let tool = operation.map_or("unknown", |operation| operation.name);
        let principal = principal(&context)?;
        let result = match operation {
            // Ferramenta oculta: o mesmo erro do REST (FR-011).
            Some(operation) if !authz::allows(&principal, operation) => {
                let error = authz::require(&principal, operation)
                    .err()
                    .unwrap_or_else(ApiError::internal);
                Ok(CallToolResponse::Complete(domain_error(&error)))
            }
            _ => self
                .tool_router
                .call(ToolCallContext::new(self, request, context))
                .await
                .map(validation_envelope),
        };
        let outcome = match &result {
            Ok(CallToolResponse::Complete(result)) if result.is_error == Some(true) => {
                "domain_error"
            }
            Ok(_) => "success",
            Err(error) if error.code == rmcp::model::ErrorCode::INVALID_PARAMS => "invalid_params",
            Err(_) => "error",
        };
        let duration_ms = started.elapsed().as_millis() as u64;
        if outcome == "error" {
            tracing::warn!(
                event = "mcp.tool.completed",
                tool,
                outcome,
                duration_ms,
                "ferramenta MCP concluida com falha"
            );
        } else {
            tracing::debug!(
                event = "mcp.tool.completed",
                tool,
                outcome,
                duration_ms,
                "ferramenta MCP concluida"
            );
        }
        result
    }
}
