//! Resources e prompt do MCP (desenho §13, 005): `engramind://context`,
//! `engramind://channels/{slug}`, `engramind://guides/{id}` e o prompt `memory-writing`. Todos
//! passam pelo `core`, com a mesma autorização das operações correspondentes.

use rmcp::{
    ErrorData,
    model::{
        GetPromptResult, Prompt, PromptMessage, ReadResourceResult, Resource, ResourceContents,
        ResourceTemplate, Role,
    },
};

use crate::{
    core::{
        Core,
        access::Purpose,
        authz,
        context::{ContextContent, ContextOutcome, GetContextQuery},
        docs::GetGuideQuery,
        operations,
        principal::{Principal, Scope},
        selection::Selection,
    },
    error::ApiError,
    store::AppStore,
};

pub const CONTEXT_URI: &str = "engramind://context";
pub const CHANNEL_PREFIX: &str = "engramind://channels/";
pub const GUIDE_PREFIX: &str = "engramind://guides/";
pub const MEMORY_WRITING: &str = "memory-writing";
pub const MEMORY_WRITING_TEXT: &str = include_str!("../../docs/prompts/memory-writing.md");

/// Erro de domínio no MCP: recurso inexistente ou fora do alcance → `-32002`; o resto,
/// requisição inválida. O envelope do domínio vai em `data`.
pub fn resource_error(error: &ApiError) -> ErrorData {
    let data = Some(error.envelope());
    let message = error.message.clone();
    if error.code.ends_with("_NOT_FOUND") || error.code == "CHANNEL_NOT_SUBSCRIBED" {
        ErrorData::resource_not_found(message, data)
    } else if error.status.is_server_error() {
        ErrorData::internal_error(message, data)
    } else {
        ErrorData::invalid_request(message, data)
    }
}

fn allows(principal: &Principal, name: &str) -> bool {
    operations::find(name).is_some_and(|operation| authz::allows(principal, operation))
}

pub async fn list(
    core: &Core<AppStore>,
    principal: &Principal,
    header: Option<&str>,
) -> Result<Vec<Resource>, ApiError> {
    let mut resources = Vec::new();
    if allows(principal, "get_context") {
        let format = core.context_format(None);
        resources.push(
            Resource::new(CONTEXT_URI, "context")
                .with_title("Contexto")
                .with_description(
                    "Bloco de memórias do token com o Engramind-Channels da conexão \
                     (context.default_format).",
                )
                .with_mime_type(format.mime()),
        );
    }
    if allows(principal, "list_memories") {
        let selection = Selection::parse(header, None)?;
        for reach in core
            .resolve(principal, &selection, Purpose::Explicit)
            .await?
            .into_iter()
            .filter(|reach| reach.scopes.contains(Scope::Read))
        {
            resources.push(
                Resource::new(
                    format!("{CHANNEL_PREFIX}{}", reach.channel.slug),
                    format!("channel:{}", reach.channel.slug),
                )
                .with_title(reach.channel.name.clone())
                .with_description("#id: título das memórias active e permanent do canal.")
                .with_mime_type("text/plain"),
            );
        }
    }
    for guide in core.docs().visible_guides(principal.is_admin()) {
        resources.push(
            Resource::new(
                format!("{GUIDE_PREFIX}{}", guide.id),
                format!("guide:{}", guide.id),
            )
            .with_title(guide.title.clone())
            .with_mime_type("text/markdown"),
        );
    }
    Ok(resources)
}

pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new(format!("{CHANNEL_PREFIX}{{slug}}"), "channel")
            .with_title("Memórias de um canal")
            .with_description(
                "#id: título das memórias active e permanent de um canal alcançado; o slug vai \
                 cru, com as barras (engramind://channels/eng/global).",
            )
            .with_mime_type("text/plain"),
        ResourceTemplate::new(format!("{GUIDE_PREFIX}{{id}}"), "guide")
            .with_title("Guia")
            .with_description("Guia da versão em execução (list_guides).")
            .with_mime_type("text/markdown"),
    ]
}

fn not_found(uri: &str) -> ApiError {
    ApiError::new("RESOURCE_NOT_FOUND", format!("Resource inexistente: {uri}"))
}

pub async fn read(
    core: &Core<AppStore>,
    principal: &Principal,
    uri: &str,
    header: Option<&str>,
) -> Result<ReadResourceResult, ApiError> {
    let (text, mime) = if uri == CONTEXT_URI {
        match core
            .get_context(principal, GetContextQuery::default(), None, header)
            .await?
        {
            ContextOutcome::Rendered(rendered) => {
                let text = match &rendered.view.content {
                    ContextContent::Text(text) => text.clone(),
                    ContextContent::Document(document) => {
                        serde_json::to_string(document).map_err(|_| ApiError::internal())?
                    }
                };
                (text, rendered.view.format.mime())
            }
            ContextOutcome::NotModified(_) => return Err(ApiError::internal()),
        }
    } else if let Some(slug) = uri
        .strip_prefix(CHANNEL_PREFIX)
        .filter(|slug| !slug.is_empty())
    {
        (
            core.channel_titles(principal, slug, header).await?,
            "text/plain",
        )
    } else if let Some(id) = uri.strip_prefix(GUIDE_PREFIX).filter(|id| !id.is_empty()) {
        (
            core.get_guide(principal, id, GetGuideQuery::default())?
                .content,
            "text/markdown",
        )
    } else {
        return Err(not_found(uri));
    };
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(text, uri).with_mime_type(mime),
    ]))
}

pub fn prompts() -> Vec<Prompt> {
    vec![Prompt::new(
        MEMORY_WRITING,
        Some("Como escrever uma boa memória (anexe ao agente que grava no Engramind)."),
        None,
    )]
}

pub fn prompt(name: &str) -> Result<GetPromptResult, ApiError> {
    if name != MEMORY_WRITING {
        return Err(ApiError::new(
            "PROMPT_NOT_FOUND",
            format!("Prompt inexistente: {name}"),
        ));
    }
    Ok(GetPromptResult::new(vec![PromptMessage::new_text(
        Role::User,
        MEMORY_WRITING_TEXT,
    )])
    .with_description("Como escrever uma boa memória"))
}
