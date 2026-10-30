//! Tipos persistidos e expostos pelo `core`: apps, agentes, tokens, canais, assinaturas,
//! auditoria, configurações e idempotência. Nenhum carrega segredo ou digest, exceto `AuthTokenRow`, que não sai do `core`.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;

use crate::core::principal::{AgentRef, AppRef, Scope, TokenKind};

/// Lê `campo: null` como `Some(None)` e campo ausente como `None` (com `#[serde(default)]`).
pub fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct App {
    #[schema(example = "app_3f9e1a7c")]
    pub id: String,
    #[schema(example = "app-a")]
    pub slug: String,
    #[schema(example = "App A")]
    pub name: String,
    pub description: Option<String>,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub created_at: String,
    /// Preenchido quando o app foi desativado: seus tokens deixam de autenticar.
    pub disabled_at: Option<String>,
}

impl App {
    pub fn reference(&self) -> AppRef {
        AppRef {
            id: self.id.clone(),
            slug: self.slug.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Agent {
    #[schema(example = "ag_5b1c9d2e")]
    pub id: String,
    pub app: AppRef,
    /// Slug do agente, único dentro do app.
    #[schema(example = "curador")]
    pub slug: String,
    /// `app/agente`, aceito no lugar do id.
    #[schema(example = "app-a/curador")]
    pub qualified_slug: String,
    #[schema(example = "Curador")]
    pub name: String,
    pub description: Option<String>,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub created_at: String,
    /// Preenchido quando o agente foi desativado: seus tokens deixam de autenticar.
    pub disabled_at: Option<String>,
}

impl Agent {
    pub fn reference(&self) -> AgentRef {
        AgentRef {
            id: self.id.clone(),
            slug: self.qualified_slug.clone(),
        }
    }
}

/// Estado de um token no momento da leitura.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TokenStatus {
    Active,
    Expired,
    Revoked,
}

/// Token sem segredo. `prefix` é a parte pública (`eng.<token_id>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Token {
    #[schema(example = "tk_3f9e1a7c")]
    pub id: String,
    #[schema(example = "eng.tk_3f9e1a7c")]
    pub prefix: String,
    pub kind: TokenKind,
    #[schema(example = json!(["read", "write"]))]
    pub scopes: Vec<Scope>,
    pub app: AppRef,
    pub agent: Option<AgentRef>,
    #[schema(example = "curador producao")]
    pub label: Option<String>,
    pub expires_at: Option<String>,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub created_at: String,
    /// Gravado em lote a cada 10 s; pode atrasar alguns segundos.
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
    pub status: TokenStatus,
}

/// Linha usada só pela autenticação: inclui o digest e o estado de app e agente.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthTokenRow {
    pub token: Token,
    pub digest: Vec<u8>,
    pub app_disabled: bool,
    pub agent_disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewApp {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppPatch {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgent {
    pub app_id: String,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
}

pub type AgentPatch = AppPatch;

#[derive(Clone, PartialEq, Eq)]
pub struct NewToken {
    /// Id escolhido (bootstrap); `None` gera um novo.
    pub id: Option<String>,
    pub app_id: String,
    pub agent_id: Option<String>,
    pub kind: TokenKind,
    pub scopes: Vec<Scope>,
    pub label: Option<String>,
    pub expires_at: Option<String>,
    pub digest: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenPatch {
    pub label: Option<Option<String>>,
    pub scopes: Option<Vec<Scope>>,
    pub expires_at: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenFilter {
    pub app_id: Option<String>,
    pub agent_id: Option<String>,
}

/// Quem fez a mudança, gravado na auditoria. Vazio no bootstrap.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Actor {
    pub app_id: Option<String>,
    pub agent_id: Option<String>,
    pub token_id: Option<String>,
}

/// Chave de idempotência a gravar na mesma transação da mutação (D-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdemRecord {
    pub token_id: String,
    pub key: String,
    pub request_hash: Vec<u8>,
    pub status: u16,
    /// Registros anteriores a este instante já expiraram e podem ser substituídos.
    pub expired_before: String,
}

/// Registro de idempotência gravado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdemRow {
    pub request_hash: Vec<u8>,
    pub response_json: String,
    pub status: u16,
    pub created_at: String,
}

/// Contexto de toda mutação: autoria, instante e idempotência.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteContext {
    pub actor: Actor,
    pub at: String,
    pub idempotency: Option<IdemRecord>,
}

/// Endereço de app ou agente: id público ou slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Id(String),
    Slug(String),
}

/// Endereço de agente: id ou `app/agente`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentLookup {
    Id(String),
    Slug { app: String, agent: String },
}

/// Evento de auditoria. `before`/`after` nunca têm segredos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AuditEvent {
    pub id: i64,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub at: String,
    pub app_id: Option<String>,
    pub agent_id: Option<String>,
    pub token_id: Option<String>,
    #[schema(example = "token.issue")]
    pub action: String,
    #[schema(example = "token")]
    pub entity: String,
    pub entity_id: Option<String>,
    #[schema(value_type = Option<Object>)]
    pub before: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>)]
    pub after: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, ToSchema, JsonSchema)]
pub struct AuditFilter {
    /// Entidade: `app`, `agent`, `token` ou `settings`.
    pub entity: Option<String>,
    /// Id da entidade (ou chave de configuração).
    pub entity_id: Option<String>,
    /// Ação, por exemplo `token.revoke`.
    pub action: Option<String>,
}

/// Contagens do `get_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct IdentityCounts {
    pub apps: u64,
    pub agents: u64,
    pub tokens: u64,
    /// Tokens não revogados e não expirados.
    pub active_tokens: u64,
}

/// Linha persistida de configuração.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingRow {
    pub key: String,
    pub value_json: String,
    pub updated_by_app: Option<String>,
    pub updated_at: String,
}

/// Mudança de configuração; `value_json = None` volta ao padrão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChange {
    pub key: String,
    pub value_json: Option<String>,
}

/// Estado do token em `now` (carimbos no formato de `ids::timestamp`, comparáveis como texto).
pub fn token_status(revoked_at: Option<&str>, expires_at: Option<&str>, now: &str) -> TokenStatus {
    if revoked_at.is_some() {
        TokenStatus::Revoked
    } else if expires_at.is_some_and(|expires_at| expires_at <= now) {
        TokenStatus::Expired
    } else {
        TokenStatus::Active
    }
}

/// Modo de uma assinatura (desenho §5): `inject` entra no contexto, na busca e nas dicas;
/// `search` só na busca e nas dicas; `manage` só quando o canal é citado explicitamente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Inject,
    Search,
    Manage,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inject => "inject",
            Self::Search => "search",
            Self::Manage => "manage",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "inject" => Some(Self::Inject),
            "search" => Some(Self::Search),
            "manage" => Some(Self::Manage),
            _ => None,
        }
    }
}

/// Alcance efetivo do token que consultou o canal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ChannelReach {
    pub mode: Mode,
    /// Escopos do token ∩ acesso da assinatura.
    #[schema(example = json!(["read"]))]
    pub access: Vec<Scope>,
    /// Prioridade da assinatura, ou a do canal quando a assinatura não a define.
    pub priority: i64,
    /// Assinatura que dá o alcance (a mais específica); `null` para token admin.
    #[schema(example = "sub_7c2e9a10")]
    pub subscription: Option<String>,
    /// Alvo dessa assinatura; `null` para token admin.
    #[schema(example = "eng/*")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Channel {
    #[schema(example = "ch_1a2b3c4d")]
    pub id: String,
    /// Hierárquico com `/`; imutável.
    #[schema(example = "eng/global")]
    pub slug: String,
    #[schema(example = "Engenharia global")]
    pub name: String,
    pub description: Option<String>,
    /// Prioridade padrão na ordem do contexto.
    pub priority: i64,
    /// Cresce a cada mudança do canal.
    #[schema(example = 1)]
    pub version: i64,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub created_at: String,
    /// Preenchido quando o canal foi arquivado.
    pub archived_at: Option<String>,
    /// Alcance do token em `list_channels` e `get_channel`; ausente nas respostas de mutação.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<ChannelReach>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Subscription {
    #[schema(example = "sub_7c2e9a10")]
    pub id: String,
    pub agent: AgentRef,
    /// Canal exato (`eng/global`) ou prefixo terminado em `*` (`eng/*`, `eng/projeto-*`).
    #[schema(example = "eng/*")]
    pub target: String,
    pub is_prefix: bool,
    pub mode: Mode,
    /// Restrição de acesso; `null` usa os escopos do token do agente.
    #[schema(example = json!(["read"]))]
    pub access: Option<Vec<Scope>>,
    /// Sobrepõe a prioridade do canal; `null` usa a do canal.
    pub priority: Option<i64>,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub created_at: String,
    /// Slugs dos canais ativos que o alvo alcança agora.
    #[serde(default)]
    #[schema(example = json!(["eng/global", "eng/projeto-atlas"]))]
    pub reaches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChannel {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub priority: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelPatch {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub priority: Option<i64>,
}

/// Assinatura a gravar. `target` vem sem o `*` final quando `is_prefix`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSubscription {
    pub agent_id: String,
    pub target: String,
    pub is_prefix: bool,
    pub mode: Mode,
    pub access: Option<Vec<Scope>>,
    pub priority: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscriptionPatch {
    pub mode: Option<Mode>,
    pub access: Option<Option<Vec<Scope>>>,
    pub priority: Option<Option<i64>>,
}

/// A rede inteira: todos os canais (com os arquivados) e todas as assinaturas, ordenados por
/// slug e por `(created_at, id)`. Base do resolvedor de acesso efetivo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Network {
    pub channels: Vec<Channel>,
    pub subscriptions: Vec<Subscription>,
}
