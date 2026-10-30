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
    /// Sobreposições de configuração por canal (`channels.settings_json`), pelo id do canal.
    pub channel_settings: ChannelOverrides,
}

/// `id do canal → chave → valor`.
pub type ChannelOverrides =
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, serde_json::Value>>;

/// Estado de uma memória (desenho §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    /// Fixada: entra primeiro no contexto e o consolidador nunca toca.
    Permanent,
    /// Fora do contexto e das dicas; buscável com `include_archived`.
    Archived,
}

impl MemoryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Permanent => "permanent",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "permanent" => Some(Self::Permanent),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// Origem do título: `pending` (provisório, aguardando o Redator), `ready` ou `fallback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TitleState {
    Pending,
    Ready,
    Fallback,
}

impl TitleState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "ready" => Some(Self::Ready),
            "fallback" => Some(Self::Fallback),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingState {
    Pending,
    Ready,
    Failed,
    Off,
}

impl EmbeddingState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "ready" => Some(Self::Ready),
            "failed" => Some(Self::Failed),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// Quem escreveu: ids sempre; slugs `null` quando o app ou o agente foi excluído.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Author {
    #[schema(example = "app_3f9e1a7c")]
    pub app_id: String,
    #[schema(example = "app-a")]
    pub app: Option<String>,
    #[schema(example = "ag_5b1c9d2e")]
    pub agent_id: Option<String>,
    /// `app/agente`.
    #[schema(example = "app-a/curador")]
    pub agent: Option<String>,
}

/// Proveniência informada por quem escreveu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    /// De onde veio, por exemplo `curator` ou `import` (até 64 caracteres).
    #[schema(example = "curator")]
    pub source: String,
    /// Referência livre, por exemplo o caso que originou a memória (até 200 caracteres).
    #[schema(example = "caso 123")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Memory {
    #[schema(example = "m_8f2a1c3d")]
    pub id: String,
    /// Slug do canal.
    #[schema(example = "eng/global")]
    pub channel: String,
    #[schema(example = "ch_1a2b3c4d")]
    pub channel_id: String,
    #[schema(example = "CONSTRAINTS")]
    pub category: String,
    #[schema(example = "Use cargo-nextest localmente")]
    pub title: String,
    pub title_state: TitleState,
    pub text: String,
    pub status: MemoryStatus,
    /// Calculada pelo consolidador; `null` até lá e depois de cada edição do texto.
    pub importance: Option<f64>,
    /// Cresce a cada mudança; base do `expected_version`.
    #[schema(example = 1)]
    pub version: i64,
    pub embedding_state: EmbeddingState,
    pub author: Author,
    pub origin: Option<Origin>,
    /// Gravado em lote a cada 5 s; pode atrasar alguns segundos.
    pub retrieval_count: i64,
    pub injected_count: i64,
    pub last_retrieved_at: Option<String>,
    pub verified_at: Option<String>,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub created_at: String,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub updated_at: String,
    pub archived_at: Option<String>,
}

/// Uma memória com `fields=title`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MemoryTitle {
    #[schema(example = "m_8f2a1c3d")]
    pub id: String,
    #[schema(example = "Use cargo-nextest localmente")]
    pub title: String,
    #[schema(example = "eng/global")]
    pub channel: String,
    #[schema(example = "CONSTRAINTS")]
    pub category: String,
    pub status: MemoryStatus,
}

impl Memory {
    pub fn title_view(&self) -> MemoryTitle {
        MemoryTitle {
            id: self.id.clone(),
            title: self.title.clone(),
            channel: self.channel.clone(),
            category: self.category.clone(),
            status: self.status,
        }
    }
}

/// Uma memória parecida com a escrita, para o agente preferir editar a duplicar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SimilarMemory {
    #[schema(example = "m_1b2c3d4e")]
    pub id: String,
    pub title: String,
    #[schema(example = "eng/global")]
    pub channel: String,
    /// Similaridade de 0 a 1.
    #[schema(example = 0.91)]
    pub score: f64,
}

/// Resposta de `create_memory` e `update_memory`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MemoryWrite {
    pub memory: Memory,
    /// `true` quando o texto já existia no canal e nada foi gravado.
    pub duplicate: bool,
    /// Até três memórias do mesmo canal com similaridade ≥ `memory.similar_threshold`.
    pub similar: Vec<SimilarMemory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MemoryRevision {
    #[schema(example = 1)]
    pub version: i64,
    pub title: String,
    pub text: String,
    #[schema(example = "CONSTRAINTS")]
    pub category: String,
    pub status: MemoryStatus,
    pub author: Author,
    /// `create`, `update`, `archive`, `restore`, `pin`, `unpin`, `move` ou `revert`.
    #[schema(example = "create")]
    pub reason: String,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub created_at: String,
}

/// Job a enfileirar junto com a mutação; o `Store` completa o payload com `memory_id` e
/// `version`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewJob {
    pub kind: &'static str,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Memória a gravar; `similar` vai junto para a resposta (e a idempotência) ser a final.
#[derive(Debug, Clone, PartialEq)]
pub struct NewMemory {
    pub channel_id: String,
    pub category: String,
    pub title: String,
    pub text: String,
    pub status: MemoryStatus,
    pub content_hash: Vec<u8>,
    pub origin: Option<Origin>,
    pub dedupe: bool,
    pub jobs: Vec<NewJob>,
    pub similar: Vec<SimilarMemory>,
}

/// Memória alvo de uma mutação: a escrita só acontece se ela ainda estiver nesse canal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryTarget {
    pub id: String,
    pub channel_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryContent {
    pub title: String,
    pub text: String,
    pub content_hash: Vec<u8>,
}

/// Edição: sem mudança efetiva, nada é gravado (Q-007). Texto ou título novos voltam
/// `title_state`/`embedding_state` para `pending` e enfileiram `jobs`; texto novo zera
/// `importance`.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryPatch {
    pub expected_version: Option<i64>,
    pub content: Option<MemoryContent>,
    pub category: Option<String>,
    pub dedupe: bool,
    pub reason: &'static str,
    pub jobs: Vec<NewJob>,
    pub similar: Vec<SimilarMemory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusAction {
    Archive,
    Restore,
    Pin,
    Unpin,
}

impl StatusAction {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::Restore => "restore",
            Self::Pin => "pin",
            Self::Unpin => "unpin",
        }
    }
}

/// Filtro de `list_memories`, já resolvido para ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryFilter {
    pub channel_ids: Vec<String>,
    pub categories: Vec<String>,
    pub statuses: Vec<MemoryStatus>,
    pub author_app_id: Option<String>,
    pub author_agent_id: Option<String>,
    /// `updated_at >= since`.
    pub since: Option<String>,
}

/// Busca textual: termos já normalizados, ordem por relevância.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextQuery {
    pub terms: Vec<String>,
    pub channel_ids: Vec<String>,
    pub categories: Vec<String>,
    pub statuses: Vec<MemoryStatus>,
    pub exclude_ids: Vec<String>,
    pub limit: u32,
}

/// Contadores de uma memória a somar em lote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalBump {
    pub memory_id: String,
    pub count: i64,
    pub at: String,
}

/// Estatísticas de um canal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MemoryCounts {
    pub total: u64,
    pub active: u64,
    pub permanent: u64,
    pub archived: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PendingCounts {
    /// Memórias com título provisório (`title_state = pending`).
    pub title: u64,
    /// Memórias sem vetor ainda (`embedding_state = pending`).
    pub embedding: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ChannelStats {
    #[schema(example = "eng/global")]
    pub channel: String,
    #[schema(example = "ch_1a2b3c4d")]
    pub channel_id: String,
    pub version: i64,
    pub memories: MemoryCounts,
    /// Memórias por categoria.
    #[schema(example = json!({ "CONSTRAINTS": 3 }))]
    pub by_category: std::collections::BTreeMap<String, u64>,
    pub pending: PendingCounts,
    /// Soma de `retrieval_count` das memórias do canal.
    pub retrievals: u64,
    /// Última mudança de memória do canal.
    pub last_memory_at: Option<String>,
}
