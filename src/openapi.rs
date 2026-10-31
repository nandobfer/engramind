use std::sync::LazyLock;

use axum::{Json, response::Redirect};
use utoipa::openapi::{
    KnownFormat, ObjectBuilder, RefOr, Required, SchemaFormat, Type,
    header::Header,
    path::{Operation, ParameterBuilder, ParameterIn},
    security::{Http, HttpAuthScheme, SecurityScheme},
    server::Server,
};
use utoipa::{Modify, OpenApi};

use crate::{
    core::{
        audit::AuditList,
        channels::{ChannelList, CreateChannelBody, UpdateChannelBody},
        context::{
            ContextCategory, ContextChannel, ContextContent, ContextCounts, ContextDocument,
            ContextFormat, ContextLine, ContextNotModified, ContextTitle, ContextView,
        },
        docs::DocText,
        identities::{
            AgentList, AppList, CreateAgentBody, CreateAppBody, IssueTokenBody, IssuedToken,
            TokenList, UpdateBody, UpdateTokenBody, WhoAmI,
        },
        memories::{
            CreateMemoryBody, Fields, MemoryHistory, MemoryItem, MemoryList, MoveMemoryBody,
            RevertMemoryBody, UpdateMemoryBody,
        },
        model::{
            Agent, App, AuditEvent, Author, Channel, ChannelReach, ChannelStats, EmbeddingState,
            IdentityCounts, Memory, MemoryCounts, MemoryRevision, MemoryStatus, MemoryTitle,
            MemoryWrite, Mode, Origin, PendingCounts, SimilarMemory, Subscription, TitleState,
            Token, TokenStatus,
        },
        principal::{AgentRef, AppRef, Scope, TokenKind},
        search::{SearchHit, SearchMemoriesBody, SearchResults},
        settings::{
            Category, ChannelSettingView, ChannelSettingsView, SettingView, SettingsView,
            UpdateChannelSettingsBody, UpdateSettingsBody,
        },
        status::{BackupStatus, DatabaseStatus, StatusView},
        subscriptions::{SubscribeBody, SubscriptionList, UpdateSubscriptionBody},
    },
    error::{ErrorBody, ErrorResponse},
    health::{LiveResponse, ReadyResponse},
    mcp::contract::{McpJsonRpcError, McpJsonRpcRequest, McpJsonRpcResponse},
    store::MigrationStatus,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Engramind API",
        description = "Rede de memórias para agentes de IA: memórias duráveis em canais, entregues por REST, MCP e CLI."
    ),
    paths(
        crate::openapi::root_docs_redirect,
        crate::health::live,
        crate::health::ready,
        crate::openapi::openapi_json_contract,
        crate::openapi::swagger_docs_contract,
        crate::rest::identities::whoami,
        crate::rest::identities::list_apps,
        crate::rest::identities::get_app,
        crate::rest::identities::create_app,
        crate::rest::identities::update_app,
        crate::rest::identities::delete_app,
        crate::rest::identities::list_agents,
        crate::rest::identities::get_agent,
        crate::rest::identities::create_agent,
        crate::rest::identities::update_agent,
        crate::rest::identities::delete_agent,
        crate::rest::identities::list_tokens,
        crate::rest::identities::issue_token,
        crate::rest::identities::update_token,
        crate::rest::identities::revoke_token,
        crate::rest::channels::list_channels,
        crate::rest::channels::get_channel,
        crate::rest::channels::create_channel,
        crate::rest::channels::update_channel,
        crate::rest::channels::archive_channel,
        crate::rest::channels::restore_channel,
        crate::rest::channels::delete_channel,
        crate::rest::channels::channel_stats,
        crate::rest::context::get_context,
        crate::rest::memories::list_memories,
        crate::rest::memories::get_memory,
        crate::rest::memories::search_memories,
        crate::rest::memories::memory_history,
        crate::rest::memories::create_memory,
        crate::rest::memories::update_memory,
        crate::rest::memories::archive_memory,
        crate::rest::memories::restore_memory,
        crate::rest::memories::pin_memory,
        crate::rest::memories::unpin_memory,
        crate::rest::memories::move_memory,
        crate::rest::memories::revert_memory,
        crate::rest::memories::delete_memory,
        crate::rest::subscriptions::list_subscriptions,
        crate::rest::subscriptions::subscribe,
        crate::rest::subscriptions::update_subscription,
        crate::rest::subscriptions::unsubscribe,
        crate::rest::settings::get_settings,
        crate::rest::settings::update_settings,
        crate::rest::settings::get_channel_settings,
        crate::rest::settings::update_channel_settings,
        crate::rest::status::get_status,
        crate::rest::audit::get_audit,
        crate::rest::docs::get_overview,
        crate::rest::docs::list_guides,
        crate::rest::docs::get_guide,
        crate::rest::docs::search_docs,
        crate::rest::docs::list_operations,
        crate::rest::docs::get_operation,
        crate::rest::docs::get_schema,
        crate::mcp::contract::mcp_contract
    ),
    components(schemas(
        ErrorResponse, ErrorBody, LiveResponse, ReadyResponse, MigrationStatus,
        TokenKind, Scope, AppRef, AgentRef, WhoAmI,
        App, AppList, CreateAppBody, UpdateBody,
        Agent, AgentList, CreateAgentBody,
        Token, TokenStatus, TokenList, IssueTokenBody, IssuedToken, UpdateTokenBody,
        Mode, Channel, ChannelReach, ChannelList, CreateChannelBody, UpdateChannelBody,
        Subscription, SubscriptionList, SubscribeBody, UpdateSubscriptionBody,
        SettingView, SettingsView, UpdateSettingsBody, Category,
        ChannelSettingView, ChannelSettingsView, UpdateChannelSettingsBody,
        MemoryStatus, TitleState, EmbeddingState, Author, Origin, Memory, MemoryTitle,
        SimilarMemory, MemoryWrite, MemoryRevision, MemoryItem, MemoryList, MemoryHistory, Fields,
        CreateMemoryBody, UpdateMemoryBody, MoveMemoryBody, RevertMemoryBody,
        SearchMemoriesBody, SearchHit, SearchResults,
        ContextFormat, ContextCounts, ContextLine, ContextCategory, ContextChannel, ContextTitle,
        ContextDocument, ContextContent, ContextView, ContextNotModified,
        ChannelStats, MemoryCounts, PendingCounts,
        StatusView, DatabaseStatus, BackupStatus, IdentityCounts,
        AuditEvent, AuditList, DocText,
        McpJsonRpcRequest, McpJsonRpcResponse, McpJsonRpcError
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "Health", description = "Verificações públicas de vida e prontidão"),
        (name = "Operations", description = "Rotas operacionais públicas (OpenAPI e Swagger UI)"),
        (name = "Authentication", description = "Identidade do token (`whoami`)"),
        (name = "Identities", description = "Apps, agentes e tokens (token admin)"),
        (name = "Channels", description = "Canais e assinaturas alcançados pelo token"),
        (name = "Network", description = "Criação e manutenção de canais e assinaturas (token admin)"),
        (name = "Context", description = "Contexto determinístico para o prompt, com `version`/ETag e `304`"),
        (name = "Memories", description = "Memórias: escrita, edição versionada, status, busca e histórico"),
        (name = "Settings", description = "Configurações funcionais (token admin)"),
        (name = "Status", description = "Estado do serviço (token admin)"),
        (name = "Audit", description = "Auditoria das mutações (token admin)"),
        (name = "Documentation", description = "Guias e contrato da versão em execução"),
        (name = "MCP", description = "Model Context Protocol em `POST /mcp`")
    )
)]
pub struct ApiDoc;

static DOCUMENT: LazyLock<utoipa::openapi::OpenApi> = LazyLock::new(ApiDoc::openapi);

/// Documento OpenAPI da versão em execução, gerado uma única vez e só no primeiro uso.
pub fn document() -> &'static utoipa::openapi::OpenApi {
    &DOCUMENT
}

/// Conteúdo de `docs/openapi.json` (snapshot versionado).
pub fn snapshot() -> String {
    let mut json = serde_json::to_string_pretty(document()).expect("OpenAPI serializa");
    json.push('\n');
    json
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi.info.license = None;
        openapi.servers = Some(vec![Server::new(".")]);
        openapi
            .components
            .get_or_insert_default()
            .add_security_scheme(
                "bearerAuth",
                SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
            );

        let request_id_parameter = ParameterBuilder::new()
            .name("x-request-id")
            .parameter_in(ParameterIn::Header)
            .required(Required::False)
            .description(Some(
                "Optional UUID correlation identifier. Invalid values are replaced.",
            ))
            .schema(Some(
                ObjectBuilder::new()
                    .schema_type(Type::String)
                    .format(Some(SchemaFormat::KnownFormat(KnownFormat::Uuid)))
                    .build(),
            ))
            .build();
        let request_id_header = Header::builder()
            .schema(
                ObjectBuilder::new()
                    .schema_type(Type::String)
                    .format(Some(SchemaFormat::KnownFormat(KnownFormat::Uuid)))
                    .build(),
            )
            .description(Some("UUID correlation identifier used by this request."))
            .build();
        let bearer_challenge = Header::builder()
            .description(Some("Bearer authentication challenge."))
            .build();

        for path in openapi.paths.paths.values_mut() {
            for operation in operations_mut(path) {
                operation
                    .parameters
                    .get_or_insert_default()
                    .push(request_id_parameter.clone());
                for response in operation.responses.responses.values_mut() {
                    if let RefOr::T(response) = response {
                        response
                            .headers
                            .insert("x-request-id".to_owned(), request_id_header.clone());
                    }
                }
                if let Some(RefOr::T(response)) = operation.responses.responses.get_mut("401") {
                    response
                        .headers
                        .insert("WWW-Authenticate".to_owned(), bearer_challenge.clone());
                }
            }
        }
    }
}

pub fn operations_mut(path: &mut utoipa::openapi::path::PathItem) -> Vec<&mut Operation> {
    [
        &mut path.get,
        &mut path.put,
        &mut path.post,
        &mut path.delete,
        &mut path.options,
        &mut path.head,
        &mut path.patch,
        &mut path.trace,
    ]
    .into_iter()
    .filter_map(Option::as_mut)
    .collect()
}

#[utoipa::path(
    get,
    path = "/openapi.json",
    operation_id = "get_openapi_document",
    tag = "Operations",
    summary = "Devolve o documento OpenAPI da versão em execução",
    security(),
    responses(
        (status = 200, description = "Documento OpenAPI em JSON", body = serde_json::Value),
        (status = 504, description = "Tempo limite da requisição", body = ErrorResponse)
    )
)]
pub async fn openapi_json_contract() -> Json<&'static utoipa::openapi::OpenApi> {
    Json(document())
}

#[utoipa::path(
    get,
    path = "/",
    operation_id = "open_swagger_ui",
    tag = "Operations",
    summary = "Redireciona para a Swagger UI",
    description = "Usa redirect relativo para que a Swagger UI funcione atrás de proxy com prefixo.",
    security(),
    responses(
        (status = 303, description = "Redirect relativo para a Swagger UI", headers(("location" = String, description = "`docs/`, relativo, preservando qualquer prefixo público"))),
        (status = 504, description = "Tempo limite da requisição", body = ErrorResponse)
    )
)]
pub async fn root_docs_redirect() -> Redirect {
    Redirect::to("docs/")
}

#[utoipa::path(
    get,
    path = "/docs",
    operation_id = "get_swagger_ui",
    tag = "Operations",
    summary = "Abre a Swagger UI",
    description = "Redireciona para `docs/`; a Swagger UI carrega `../openapi.json`.",
    security(),
    responses(
        (status = 303, description = "Redirect relativo para o índice da Swagger UI", headers(("location" = String, description = "`docs/`, relativo, preservando qualquer prefixo público"))),
        (status = 504, description = "Tempo limite da requisição", body = ErrorResponse)
    )
)]
pub async fn swagger_docs_contract() {}
