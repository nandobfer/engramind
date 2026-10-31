//! Catálogo único das operações (D-002): nome base (= `operationId` = ferramenta MCP), tag,
//! tipo e escopo exigidos, hints MCP, rota REST e caminho da CLI. Autorização, MCP, CLI,
//! `whoami` e os testes de contrato leem daqui.

use super::principal::Scope;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredKind {
    /// Qualquer token (`agent` ou `admin`).
    Any,
    /// Só token `admin`.
    Admin,
}

impl RequiredKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Admin => "admin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
    pub name: &'static str,
    pub tag: &'static str,
    /// Resumo em pt-BR; igual ao `summary` do OpenAPI e à descrição da ferramenta MCP.
    pub summary: &'static str,
    pub kind: RequiredKind,
    /// `None`: qualquer token autenticado.
    pub scope: Option<Scope>,
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub method: &'static str,
    pub path: &'static str,
    pub cli: &'static [&'static str],
}

pub const TAG_AUTHENTICATION: &str = "Authentication";
pub const TAG_IDENTITIES: &str = "Identities";
pub const TAG_CHANNELS: &str = "Channels";
pub const TAG_NETWORK: &str = "Network";
pub const TAG_CONTEXT: &str = "Context";
pub const TAG_MEMORIES: &str = "Memories";
pub const TAG_SEARCH: &str = "Search";
pub const TAG_EMBEDDINGS: &str = "Embeddings";
pub const TAG_SETTINGS: &str = "Settings";
pub const TAG_STATUS: &str = "Status";
pub const TAG_JOBS: &str = "Jobs";
pub const TAG_AUDIT: &str = "Audit";
pub const TAG_INTEGRATION: &str = "Integration";
pub const TAG_DOCUMENTATION: &str = "Documentation";

const fn read(
    name: &'static str,
    tag: &'static str,
    summary: &'static str,
    kind: RequiredKind,
    path: &'static str,
    cli: &'static [&'static str],
) -> OperationSpec {
    OperationSpec {
        name,
        tag,
        summary,
        kind,
        scope: Some(Scope::Read),
        read_only: true,
        destructive: false,
        idempotent: true,
        method: "GET",
        path,
        cli,
    }
}

const fn public_read(
    name: &'static str,
    tag: &'static str,
    summary: &'static str,
    path: &'static str,
    cli: &'static [&'static str],
) -> OperationSpec {
    OperationSpec {
        scope: None,
        ..read(name, tag, summary, RequiredKind::Any, path, cli)
    }
}

#[allow(clippy::too_many_arguments)]
const fn mutation(
    name: &'static str,
    tag: &'static str,
    summary: &'static str,
    scope: Scope,
    destructive: bool,
    idempotent: bool,
    method: &'static str,
    path: &'static str,
    cli: &'static [&'static str],
) -> OperationSpec {
    OperationSpec {
        name,
        tag,
        summary,
        kind: RequiredKind::Admin,
        scope: Some(scope),
        read_only: false,
        destructive,
        idempotent,
        method,
        path,
        cli,
    }
}

/// Mutação de qualquer token; o acesso efetivo no canal é conferido no `core`.
#[allow(clippy::too_many_arguments)]
const fn any_mutation(
    name: &'static str,
    tag: &'static str,
    summary: &'static str,
    scope: Scope,
    destructive: bool,
    idempotent: bool,
    method: &'static str,
    path: &'static str,
    cli: &'static [&'static str],
) -> OperationSpec {
    OperationSpec {
        kind: RequiredKind::Any,
        ..mutation(
            name,
            tag,
            summary,
            scope,
            destructive,
            idempotent,
            method,
            path,
            cli,
        )
    }
}

use RequiredKind::Admin;

pub const OPERATIONS: &[OperationSpec] = &[
    public_read(
        "whoami",
        TAG_AUTHENTICATION,
        "Identifica o token e lista as operações permitidas",
        "/v1/whoami",
        &["whoami"],
    ),
    read(
        "list_apps",
        TAG_IDENTITIES,
        "Lista os apps",
        Admin,
        "/v1/apps",
        &["apps", "list"],
    ),
    read(
        "get_app",
        TAG_IDENTITIES,
        "Mostra um app",
        Admin,
        "/v1/apps/{app}",
        &["apps", "get"],
    ),
    mutation(
        "create_app",
        TAG_IDENTITIES,
        "Cria um app",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/apps",
        &["apps", "create"],
    ),
    mutation(
        "update_app",
        TAG_IDENTITIES,
        "Altera ou desativa um app",
        Scope::Write,
        false,
        true,
        "PATCH",
        "/v1/apps/{app}",
        &["apps", "update"],
    ),
    mutation(
        "delete_app",
        TAG_IDENTITIES,
        "Exclui um app com seus agentes e tokens",
        Scope::Delete,
        true,
        true,
        "DELETE",
        "/v1/apps/{app}",
        &["apps", "delete"],
    ),
    read(
        "list_agents",
        TAG_IDENTITIES,
        "Lista os agentes",
        Admin,
        "/v1/agents",
        &["agents", "list"],
    ),
    read(
        "get_agent",
        TAG_IDENTITIES,
        "Mostra um agente",
        Admin,
        "/v1/agents/{agent}",
        &["agents", "get"],
    ),
    mutation(
        "create_agent",
        TAG_IDENTITIES,
        "Cria um agente num app",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/agents",
        &["agents", "create"],
    ),
    mutation(
        "update_agent",
        TAG_IDENTITIES,
        "Altera ou desativa um agente",
        Scope::Write,
        false,
        true,
        "PATCH",
        "/v1/agents/{agent}",
        &["agents", "update"],
    ),
    mutation(
        "delete_agent",
        TAG_IDENTITIES,
        "Exclui um agente com seus tokens",
        Scope::Delete,
        true,
        true,
        "DELETE",
        "/v1/agents/{agent}",
        &["agents", "delete"],
    ),
    read(
        "list_tokens",
        TAG_IDENTITIES,
        "Lista os tokens, sem segredos",
        Admin,
        "/v1/tokens",
        &["tokens", "list"],
    ),
    mutation(
        "issue_token",
        TAG_IDENTITIES,
        "Emite um token; o segredo aparece só nesta resposta",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/tokens",
        &["tokens", "issue"],
    ),
    mutation(
        "update_token",
        TAG_IDENTITIES,
        "Altera rótulo, escopos ou expiração de um token",
        Scope::Write,
        false,
        true,
        "PATCH",
        "/v1/tokens/{token}",
        &["tokens", "update"],
    ),
    mutation(
        "revoke_token",
        TAG_IDENTITIES,
        "Revoga um token imediatamente",
        Scope::Delete,
        true,
        true,
        "POST",
        "/v1/tokens/{token}/revoke",
        &["tokens", "revoke"],
    ),
    read(
        "list_channels",
        TAG_CHANNELS,
        "Lista os canais alcançados pelo token",
        RequiredKind::Any,
        "/v1/channels",
        &["channels", "list"],
    ),
    read(
        "get_channel",
        TAG_CHANNELS,
        "Mostra um canal alcançado pelo token",
        RequiredKind::Any,
        "/v1/channels/{channel}",
        &["channels", "get"],
    ),
    read(
        "channel_stats",
        TAG_CHANNELS,
        "Contagens das memórias de um canal",
        RequiredKind::Any,
        "/v1/channels/{channel}/stats",
        &["channels", "stats"],
    ),
    mutation(
        "create_channel",
        TAG_NETWORK,
        "Cria um canal",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/channels",
        &["channels", "create"],
    ),
    mutation(
        "update_channel",
        TAG_NETWORK,
        "Altera nome, descrição ou prioridade de um canal",
        Scope::Write,
        false,
        true,
        "PATCH",
        "/v1/channels/{channel}",
        &["channels", "update"],
    ),
    mutation(
        "archive_channel",
        TAG_NETWORK,
        "Arquiva um canal",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/channels/{channel}/archive",
        &["channels", "archive"],
    ),
    mutation(
        "restore_channel",
        TAG_NETWORK,
        "Restaura um canal arquivado",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/channels/{channel}/restore",
        &["channels", "restore"],
    ),
    mutation(
        "delete_channel",
        TAG_NETWORK,
        "Exclui um canal arquivado",
        Scope::Delete,
        true,
        true,
        "DELETE",
        "/v1/channels/{channel}",
        &["channels", "delete"],
    ),
    read(
        "list_subscriptions",
        TAG_CHANNELS,
        "Lista as assinaturas (token agent: só as próprias)",
        RequiredKind::Any,
        "/v1/subscriptions",
        &["subscriptions", "list"],
    ),
    mutation(
        "subscribe",
        TAG_NETWORK,
        "Assina um agente a um canal ou prefixo",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/subscriptions",
        &["subscriptions", "add"],
    ),
    mutation(
        "update_subscription",
        TAG_NETWORK,
        "Altera modo, acesso ou prioridade de uma assinatura",
        Scope::Write,
        false,
        true,
        "PATCH",
        "/v1/subscriptions/{subscription}",
        &["subscriptions", "update"],
    ),
    mutation(
        "unsubscribe",
        TAG_NETWORK,
        "Cancela uma assinatura",
        Scope::Write,
        true,
        true,
        "DELETE",
        "/v1/subscriptions/{subscription}",
        &["subscriptions", "remove"],
    ),
    read(
        "get_context",
        TAG_CONTEXT,
        "Bloco de memórias pronto para o prompt",
        RequiredKind::Any,
        "/v1/context",
        &["context"],
    ),
    read(
        "list_memories",
        TAG_MEMORIES,
        "Lista as memórias dos canais alcançados",
        RequiredKind::Any,
        "/v1/memories",
        &["memories", "list"],
    ),
    read(
        "get_memory",
        TAG_MEMORIES,
        "Mostra uma memória",
        RequiredKind::Any,
        "/v1/memories/{memory}",
        &["memories", "get"],
    ),
    OperationSpec {
        method: "POST",
        ..read(
            "search_memories",
            TAG_SEARCH,
            "Busca memórias por sentido e por texto",
            RequiredKind::Any,
            "/v1/memories/search",
            &["memories", "search"],
        )
    },
    OperationSpec {
        method: "POST",
        ..read(
            "get_hints",
            TAG_SEARCH,
            "Dicas baratas (id, título e canal) para um texto, sem LLM",
            RequiredKind::Any,
            "/v1/hints",
            &["hints"],
        )
    },
    read(
        "memory_history",
        TAG_MEMORIES,
        "Lista as revisões de uma memória",
        RequiredKind::Any,
        "/v1/memories/{memory}/history",
        &["memories", "history"],
    ),
    any_mutation(
        "create_memory",
        TAG_MEMORIES,
        "Grava uma memória num canal",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/memories",
        &["memories", "create"],
    ),
    any_mutation(
        "update_memory",
        TAG_MEMORIES,
        "Edita o texto ou a categoria de uma memória",
        Scope::Write,
        false,
        true,
        "PATCH",
        "/v1/memories/{memory}",
        &["memories", "update"],
    ),
    any_mutation(
        "archive_memory",
        TAG_MEMORIES,
        "Arquiva uma memória",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/memories/{memory}/archive",
        &["memories", "archive"],
    ),
    any_mutation(
        "restore_memory",
        TAG_MEMORIES,
        "Restaura uma memória arquivada",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/memories/{memory}/restore",
        &["memories", "restore"],
    ),
    any_mutation(
        "pin_memory",
        TAG_MEMORIES,
        "Fixa uma memória (status permanent)",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/memories/{memory}/pin",
        &["memories", "pin"],
    ),
    any_mutation(
        "unpin_memory",
        TAG_MEMORIES,
        "Desafixa uma memória",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/memories/{memory}/unpin",
        &["memories", "unpin"],
    ),
    any_mutation(
        "move_memory",
        TAG_MEMORIES,
        "Move uma memória para outro canal",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/memories/{memory}/move",
        &["memories", "move"],
    ),
    any_mutation(
        "revert_memory",
        TAG_MEMORIES,
        "Restaura o conteúdo de uma versão anterior numa revisão nova",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/memories/{memory}/revert",
        &["memories", "revert"],
    ),
    any_mutation(
        "delete_memory",
        TAG_MEMORIES,
        "Exclui uma memória, mantendo a última revisão",
        Scope::Delete,
        true,
        true,
        "DELETE",
        "/v1/memories/{memory}",
        &["memories", "delete"],
    ),
    mutation(
        "import_memories",
        TAG_INTEGRATION,
        "Importa memórias em lote preservando data, autoria e origem",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/memories/import",
        &["memories", "import"],
    ),
    read(
        "get_settings",
        TAG_SETTINGS,
        "Lê as configurações com valores padrão",
        Admin,
        "/v1/settings",
        &["settings", "get"],
    ),
    mutation(
        "update_settings",
        TAG_SETTINGS,
        "Altera uma ou mais configurações",
        Scope::Write,
        false,
        true,
        "PUT",
        "/v1/settings",
        &["settings", "set"],
    ),
    read(
        "get_channel_settings",
        TAG_SETTINGS,
        "Lê as configurações de um canal com as sobreposições",
        Admin,
        "/v1/channels/{channel}/settings",
        &["channels", "settings", "get"],
    ),
    mutation(
        "update_channel_settings",
        TAG_SETTINGS,
        "Altera as sobreposições de configuração de um canal",
        Scope::Write,
        false,
        true,
        "PUT",
        "/v1/channels/{channel}/settings",
        &["channels", "settings", "set"],
    ),
    mutation(
        "set_secret",
        TAG_SETTINGS,
        "Grava ou apaga um segredo de configuração",
        Scope::Write,
        false,
        true,
        "PUT",
        "/v1/secrets/{key}",
        &["secrets", "set"],
    ),
    mutation(
        "test_redator",
        TAG_SETTINGS,
        "Testa o Redator com um texto, sem gravar a memória",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/settings/redator/test",
        &["settings", "test-redator"],
    ),
    mutation(
        "test_embedding",
        TAG_EMBEDDINGS,
        "Testa a configuração de embedding com um texto, sem gravar nada",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/settings/embedding/test",
        &["settings", "test-embedding"],
    ),
    mutation(
        "list_embedding_models",
        TAG_EMBEDDINGS,
        "Lista os modelos de embedding de um provedor",
        Scope::Write,
        false,
        false,
        "POST",
        "/v1/settings/embedding/models",
        &["settings", "embedding-models"],
    ),
    mutation(
        "reembed_all",
        TAG_EMBEDDINGS,
        "Revetoriza todas as memórias com o modelo vigente",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/embeddings/reembed",
        &["embeddings", "reembed"],
    ),
    read(
        "list_jobs",
        TAG_JOBS,
        "Lista os jobs da fila, do mais recente ao mais antigo",
        Admin,
        "/v1/jobs",
        &["jobs", "list"],
    ),
    mutation(
        "retry_job",
        TAG_JOBS,
        "Recoloca na fila um job que falhou",
        Scope::Write,
        false,
        true,
        "POST",
        "/v1/jobs/{job}/retry",
        &["jobs", "retry"],
    ),
    read(
        "list_llm_calls",
        TAG_JOBS,
        "Lista as chamadas ao LLM, da mais recente à mais antiga",
        Admin,
        "/v1/llm-calls",
        &["llm-calls", "list"],
    ),
    read(
        "get_status",
        TAG_STATUS,
        "Mostra versão, uptime, banco, backup, fila, Redator, embeddings e webhooks",
        Admin,
        "/v1/status",
        &["status"],
    ),
    read(
        "get_audit",
        TAG_AUDIT,
        "Lista os eventos de auditoria, do mais recente ao mais antigo",
        Admin,
        "/v1/audit",
        &["audit", "list"],
    ),
    read(
        "get_usage",
        TAG_STATUS,
        "Agrega o uso (contexto, busca e dicas) por dia, agente, operação ou canal",
        Admin,
        "/v1/usage",
        &["usage"],
    ),
    public_read(
        "get_overview",
        TAG_DOCUMENTATION,
        "Visão geral do serviço e dos guias disponíveis ao token",
        "/v1/docs/overview",
        &["docs", "overview"],
    ),
    public_read(
        "list_guides",
        TAG_DOCUMENTATION,
        "Lista os guias com seções e operações",
        "/v1/docs/guides",
        &["docs", "guides"],
    ),
    public_read(
        "get_guide",
        TAG_DOCUMENTATION,
        "Lê um guia ou uma seção dele",
        "/v1/docs/guides/{guide}",
        &["docs", "guide"],
    ),
    public_read(
        "search_docs",
        TAG_DOCUMENTATION,
        "Busca nos guias e nas operações por palavras-chave",
        "/v1/docs/search",
        &["docs", "search"],
    ),
    public_read(
        "list_operations",
        TAG_DOCUMENTATION,
        "Lista as operações visíveis ao token",
        "/v1/docs/operations",
        &["docs", "operations"],
    ),
    public_read(
        "get_operation",
        TAG_DOCUMENTATION,
        "Contrato completo de uma operação",
        "/v1/docs/operations/{operation}",
        &["docs", "operation"],
    ),
    public_read(
        "get_schema",
        TAG_DOCUMENTATION,
        "Esquema OpenAPI com as referências",
        "/v1/docs/schemas/{schema}",
        &["docs", "schema"],
    ),
];

pub fn find(name: &str) -> Option<&'static OperationSpec> {
    OPERATIONS.iter().find(|operation| operation.name == name)
}

/// Operação servida pela rota REST (`método` + caminho parametrizado).
pub fn by_route(method: &str, path: &str) -> Option<&'static OperationSpec> {
    OPERATIONS
        .iter()
        .find(|operation| operation.path == path && operation.method.eq_ignore_ascii_case(method))
}

/// Operação pelo nome; nomes fora do catálogo são erro de programação.
pub fn spec(name: &str) -> &'static OperationSpec {
    find(name).unwrap_or_else(|| panic!("operacao fora do catalogo: {name}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn names_routes_and_cli_paths_are_unique() {
        let mut names = BTreeSet::new();
        let mut routes = BTreeSet::new();
        let mut cli = BTreeSet::new();
        for operation in OPERATIONS {
            assert!(names.insert(operation.name), "{}", operation.name);
            assert!(
                routes.insert((operation.method, operation.path)),
                "{}",
                operation.name
            );
            assert!(cli.insert(operation.cli), "{}", operation.name);
            assert!(operation.path.starts_with("/v1/"), "{}", operation.name);
            assert!(
                operation
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_')
            );
        }
    }

    #[test]
    fn hints_are_consistent_with_the_method_and_scope() {
        for operation in OPERATIONS {
            // `search_memories` e `get_hints` são leituras por POST (o corpo leva o texto).
            assert_eq!(
                operation.read_only,
                operation.method == "GET"
                    || matches!(operation.name, "search_memories" | "get_hints"),
                "{}",
                operation.name
            );
            if operation.read_only {
                assert!(!operation.destructive && operation.idempotent);
                assert!(matches!(operation.scope, None | Some(Scope::Read)));
            } else {
                assert_eq!(
                    operation.kind == RequiredKind::Any,
                    operation.tag == TAG_MEMORIES,
                    "{}",
                    operation.name
                );
            }
            if operation.scope == Some(Scope::Delete) {
                assert!(operation.destructive, "{}", operation.name);
            }
        }
    }
}
