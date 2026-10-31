//! Ferramentas MCP: uma por operação do catálogo, com os mesmos parâmetros, escopos e códigos
//! de erro do REST. `tools/list` mostra só as permitidas ao token (FR-011) e os hints vêm de
//! `OPERATIONS` (FR-012).

use std::{collections::BTreeMap, time::Instant};

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
        GetPromptRequestParams, GetPromptResponse, Implementation, ListPromptsResult,
        ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResponse, ServerCapabilities, ServerConfig, Tool,
        ToolAnnotations,
    },
    service::{RequestContext, RoleServer},
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::resources;
use crate::{
    core::{
        Core,
        audit::AuditQuery,
        authz,
        channels::{CreateChannelBody, ListChannelsQuery, UpdateChannelBody},
        consolidation::RunConsolidationBody,
        context::{ContextFormat, ContextOutcome, GetContextQuery},
        docs::{GetGuideQuery, ListOperationsQuery, SearchDocsQuery},
        embeddings::{ListEmbeddingModelsBody, TestEmbeddingBody},
        hints::HintsBody,
        identities::{
            CreateAgentBody, CreateAppBody, IssueTokenBody, ListAgentsQuery, ListQuery,
            ListTokensQuery, UpdateAppBody, UpdateBody, UpdateTokenBody,
        },
        import::ImportMemoriesBody,
        jobs::{ListJobsQuery, ListLlmCallsQuery},
        memories::{
            CreateMemoryBody, HistoryQuery, ListMemoriesQuery, MoveMemoryBody, RevertMemoryBody,
            UpdateMemoryBody,
        },
        model::double_option,
        operations::{self, OPERATIONS},
        principal::Principal,
        redator::TestRedatorBody,
        search::SearchMemoriesBody,
        secrets::SetSecretBody,
        settings::{GetSettingsQuery, UpdateChannelSettingsBody, UpdateSettingsBody},
        subscriptions::{ListSubscriptionsQuery, SubscribeBody, UpdateSubscriptionBody},
        usage_report::GetUsageQuery,
    },
    error::ApiError,
    metrics::OperationSlot,
    rest::ChannelsHeader,
    store::AppStore,
};

pub const SERVER_NAME: &str = "engramind";

const INSTRUCTIONS: &str = "Engramind: rede de memorias para agentes de IA. As ferramentas sao as \
operacoes da API, com os mesmos nomes, parametros, escopos e erros do REST; so aparecem as que este \
token pode usar. O cabecalho Engramind-Channels da conexao restringe os canais. Comece por \
whoami e get_overview; use get_guide e search_docs para as regras e \
get_operation/get_schema para o contrato exato. get_context (ou o resource engramind://context) \
traz o bloco de memorias para o prompt. Erros de dominio voltam com isError e o envelope \
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
    pub body: UpdateAppBody,
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

/// Sem `Debug`: carrega o segredo em claro.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSecretRequest {
    /// Chave do segredo, por exemplo `redator.api_key`.
    pub key: String,
    /// Valor do segredo (até 4096 caracteres); `null` apaga.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub value: Option<Option<String>>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

/// Sem `Debug`: a proposta pode carregar `redator.api_key` em claro.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestRedatorRequest {
    /// Texto de exemplo, com os limites de `memory.text_min`/`text_max`.
    pub text: String,
    /// Mapa parcial `redator.* → valor` usado só neste teste.
    #[serde(default)]
    pub settings: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

/// Sem `Debug`: a proposta pode carregar `embedding.api_key` em claro.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestEmbeddingRequest {
    /// Texto de exemplo (1 a 2000 caracteres).
    pub text: String,
    /// Mapa parcial `embedding.* → valor` usado só neste teste.
    #[serde(default)]
    pub settings: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

/// Sem `Debug`: pode carregar a chave do provedor em claro.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListEmbeddingModelsRequest {
    /// `local`, `openai_compatible` ou `ollama` (`gemini` e `off` não listam).
    pub provider: String,
    /// URL do provedor remoto; vazia usa `embedding.base_url`.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Chave em claro (não é gravada); vazia usa `embedding.api_key`.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunConsolidationRequest {
    #[serde(flatten)]
    pub body: RunConsolidationBody,
    /// Mesmo efeito do cabeçalho `Idempotency-Key`.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportMemoriesRequest {
    #[serde(flatten)]
    pub body: ImportMemoriesBody,
    /// Obrigatória (`IDEMPOTENCY_KEY_REQUIRED`): mesmo efeito do cabeçalho `Idempotency-Key`.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetryJobRequest {
    /// Id numérico do job.
    pub job: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateChannelSettingsRequest {
    /// Id (`ch_…`) ou slug do canal (`eng/global`).
    pub channel: String,
    /// Mapa parcial `chave → valor` de sobreposições; `null` remove a sobreposição.
    pub settings: BTreeMap<String, Value>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRequest {
    /// Id da memória (`m_…`).
    pub memory: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryHistoryRequest {
    /// Id da memória (`m_…`).
    pub memory: String,
    #[serde(flatten)]
    pub query: HistoryQuery,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationRequest {
    /// Id da memória (`m_…`).
    pub memory: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateMemoryRequest {
    #[serde(flatten)]
    pub body: CreateMemoryBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateMemoryRequest {
    /// Id da memória (`m_…`).
    pub memory: String,
    #[serde(flatten)]
    pub body: UpdateMemoryBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MoveMemoryRequest {
    /// Id da memória (`m_…`).
    pub memory: String,
    #[serde(flatten)]
    pub body: MoveMemoryBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RevertMemoryRequest {
    /// Id da memória (`m_…`).
    pub memory: String,
    #[serde(flatten)]
    pub body: RevertMemoryBody,
    #[serde(flatten)]
    pub idempotency: Idempotency,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetContextRequest {
    /// Slugs e prefixos (`eng/global,eng/projeto-*`) separados por vírgula; restringe dentro do
    /// que o token alcança e do cabeçalho `Engramind-Channels`.
    #[serde(default)]
    pub channels: Option<String>,
    /// `tagged`, `markdown` ou `json`; padrão `context.default_format`.
    #[serde(default)]
    pub format: Option<ContextFormat>,
    /// Mesmo efeito do cabeçalho `If-None-Match` do REST: com a `version` atual, a resposta é
    /// `{ not_modified: true, version }`.
    #[serde(default)]
    pub if_none_match: Option<String>,
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

/// Evento de conclusão de resources (sem URI nem conteúdo): `DEBUG`, ou `WARN` em falha
/// interna.
fn completed<T>(method: &'static str, result: &Result<T, ApiError>, started: Instant) {
    let duration_ms = started.elapsed().as_millis() as u64;
    match result {
        Err(error) if error.status.is_server_error() => tracing::warn!(
            event = "mcp.resource.completed",
            method,
            outcome = "error",
            code = error.code,
            duration_ms,
            "resource MCP concluido com falha"
        ),
        Err(error) => tracing::debug!(
            event = "mcp.resource.completed",
            method,
            outcome = "domain_error",
            code = error.code,
            duration_ms,
            "resource MCP concluido"
        ),
        Ok(_) => tracing::debug!(
            event = "mcp.resource.completed",
            method,
            outcome = "success",
            duration_ms,
            "resource MCP concluido"
        ),
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
    async fn channel_stats(
        &self,
        Parameters(request): Parameters<ChannelRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .channel_stats(&principal, &request.channel, header.as_deref())
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result)
    }

    #[tool]
    async fn get_context(
        &self,
        Parameters(request): Parameters<GetContextRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let query = GetContextQuery {
            channels: request.channels,
            format: request.format,
        };
        let result = match channels_header(&context) {
            Ok(header) => self
                .core
                .get_context(
                    &principal,
                    query,
                    request.if_none_match.as_deref(),
                    header.as_deref(),
                )
                .await
                .and_then(|outcome| {
                    match outcome {
                        ContextOutcome::NotModified(not_modified) => {
                            serde_json::to_value(not_modified)
                        }
                        ContextOutcome::Rendered(rendered) => serde_json::to_value(&rendered.view),
                    }
                    .map_err(|_| ApiError::internal())
                }),
            Err(error) => Err(error),
        };
        reply(result)
    }

    #[tool]
    async fn list_memories(
        &self,
        Parameters(query): Parameters<ListMemoriesQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .list_memories(&principal, query, header.as_deref())
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result)
    }

    #[tool]
    async fn get_memory(
        &self,
        Parameters(request): Parameters<MemoryRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .get_memory(&principal, &request.memory, header.as_deref())
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result)
    }

    #[tool]
    async fn search_memories(
        &self,
        Parameters(body): Parameters<SearchMemoriesBody>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .search_memories(&principal, body, header.as_deref())
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result)
    }

    #[tool]
    async fn get_hints(
        &self,
        Parameters(body): Parameters<HintsBody>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .get_hints(&principal, body, header.as_deref())
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result)
    }

    #[tool]
    async fn memory_history(
        &self,
        Parameters(request): Parameters<MemoryHistoryRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .memory_history(
                        &principal,
                        &request.memory,
                        request.query,
                        header.as_deref(),
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result)
    }

    #[tool]
    async fn create_memory(
        &self,
        Parameters(request): Parameters<CreateMemoryRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .create_memory(&principal, request.body, header.as_deref(), key)
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn update_memory(
        &self,
        Parameters(request): Parameters<UpdateMemoryRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .update_memory(
                        &principal,
                        &request.memory,
                        request.body,
                        header.as_deref(),
                        key,
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn archive_memory(
        &self,
        Parameters(request): Parameters<MemoryMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .archive_memory(&principal, &request.memory, header.as_deref(), key)
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn restore_memory(
        &self,
        Parameters(request): Parameters<MemoryMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .restore_memory(&principal, &request.memory, header.as_deref(), key)
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn pin_memory(
        &self,
        Parameters(request): Parameters<MemoryMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .pin_memory(&principal, &request.memory, header.as_deref(), key)
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn mark_verified(
        &self,
        Parameters(request): Parameters<MemoryMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .mark_verified(&principal, &request.memory, header.as_deref(), key)
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn unpin_memory(
        &self,
        Parameters(request): Parameters<MemoryMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .unpin_memory(&principal, &request.memory, header.as_deref(), key)
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn move_memory(
        &self,
        Parameters(request): Parameters<MoveMemoryRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .move_memory(
                        &principal,
                        &request.memory,
                        request.body,
                        header.as_deref(),
                        key,
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn revert_memory(
        &self,
        Parameters(request): Parameters<RevertMemoryRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .revert_memory(
                        &principal,
                        &request.memory,
                        request.body,
                        header.as_deref(),
                        key,
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn delete_memory(
        &self,
        Parameters(request): Parameters<MemoryMutationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        let key = request.idempotency_key.as_deref();
        let result = match channels_header(&context) {
            Ok(header) => {
                self.core
                    .delete_memory(&principal, &request.memory, header.as_deref(), key)
                    .await
            }
            Err(error) => Err(error),
        };
        reply(result.map(|mutation| mutation.value))
    }

    #[tool]
    async fn get_channel_settings(
        &self,
        Parameters(request): Parameters<ChannelRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .get_channel_settings(&principal, &request.channel)
                .await,
        )
    }

    #[tool]
    async fn update_channel_settings(
        &self,
        Parameters(request): Parameters<UpdateChannelSettingsRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .update_channel_settings(
                    &principal,
                    &request.channel,
                    UpdateChannelSettingsBody(request.settings),
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
    async fn set_secret(
        &self,
        Parameters(request): Parameters<SetSecretRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .set_secret(
                    &principal,
                    &request.key,
                    SetSecretBody {
                        value: request.value,
                    },
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn test_redator(
        &self,
        Parameters(request): Parameters<TestRedatorRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .test_redator(
                    &principal,
                    TestRedatorBody {
                        text: request.text,
                        settings: request.settings,
                    },
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn test_embedding(
        &self,
        Parameters(request): Parameters<TestEmbeddingRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .test_embedding(
                    &principal,
                    TestEmbeddingBody {
                        text: request.text,
                        settings: request.settings,
                    },
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn list_embedding_models(
        &self,
        Parameters(request): Parameters<ListEmbeddingModelsRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .list_embedding_models(
                    &principal,
                    ListEmbeddingModelsBody {
                        provider: request.provider,
                        base_url: request.base_url,
                        api_key: request.api_key,
                    },
                    request.idempotency_key.as_deref(),
                )
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn reembed_all(
        &self,
        Parameters(request): Parameters<Idempotency>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .reembed_all(&principal, request.idempotency_key.as_deref())
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn run_consolidation(
        &self,
        Parameters(request): Parameters<RunConsolidationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .run_consolidation(&principal, request.body, request.idempotency_key.as_deref())
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn list_jobs(
        &self,
        Parameters(query): Parameters<ListJobsQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.list_jobs(&principal, query).await)
    }

    #[tool]
    async fn retry_job(
        &self,
        Parameters(request): Parameters<RetryJobRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .retry_job(&principal, &request.job, request.idempotency_key.as_deref())
                .await
                .map(|mutation| mutation.value),
        )
    }

    #[tool]
    async fn list_llm_calls(
        &self,
        Parameters(query): Parameters<ListLlmCallsQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.list_llm_calls(&principal, query).await)
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
    async fn get_usage(
        &self,
        Parameters(query): Parameters<GetUsageQuery>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(self.core.get_usage(&principal, query).await)
    }

    #[tool]
    async fn import_memories(
        &self,
        Parameters(request): Parameters<ImportMemoriesRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = principal(&context)?;
        reply(
            self.core
                .import_memories(&principal, request.body, request.idempotency_key.as_deref())
                .await
                .map(|mutation| mutation.value),
        )
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
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
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

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let principal = principal(&context)?;
        let started = Instant::now();
        let result = match channels_header(&context) {
            Ok(header) => resources::list(&self.core, &principal, header.as_deref()).await,
            Err(error) => Err(error),
        };
        completed("resources/list", &result, started);
        result
            .map(ListResourcesResult::with_all_items)
            .map_err(|error| resources::resource_error(&error))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        principal(&context)?;
        Ok(ListResourceTemplatesResult::with_all_items(
            resources::templates(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let principal = principal(&context)?;
        let started = Instant::now();
        let result = match channels_header(&context) {
            Ok(header) => {
                resources::read(&self.core, &principal, &request.uri, header.as_deref()).await
            }
            Err(error) => Err(error),
        };
        completed("resources/read", &result, started);
        result
            .map(ReadResourceResponse::from)
            .map_err(|error| resources::resource_error(&error))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        principal(&context)?;
        Ok(ListPromptsResult::with_all_items(resources::prompts()))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        principal(&context)?;
        resources::prompt(&request.name)
            .map(GetPromptResponse::from)
            .map_err(|error| resources::resource_error(&error))
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
        if let (Some(operation), Some(slot)) = (
            operation,
            context
                .extensions
                .get::<axum::http::request::Parts>()
                .and_then(|parts| parts.extensions.get::<OperationSlot>()),
        ) {
            slot.set(operation.name);
        }
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
