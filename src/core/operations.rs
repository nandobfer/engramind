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
pub const TAG_SETTINGS: &str = "Settings";
pub const TAG_STATUS: &str = "Status";
pub const TAG_AUDIT: &str = "Audit";
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
        "get_status",
        TAG_STATUS,
        "Mostra versão, banco, backup e contagens",
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
            assert_eq!(
                operation.read_only,
                operation.method == "GET",
                "{}",
                operation.name
            );
            if operation.read_only {
                assert!(!operation.destructive && operation.idempotent);
                assert!(matches!(operation.scope, None | Some(Scope::Read)));
            } else {
                assert_eq!(operation.kind, RequiredKind::Admin, "{}", operation.name);
            }
            if operation.scope == Some(Scope::Delete) {
                assert!(operation.destructive, "{}", operation.name);
            }
        }
    }
}
