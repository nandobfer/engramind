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
    /// Destino dos eventos do app (desenho §17); `null` sem `webhook_url`.
    pub webhook: Option<AppWebhook>,
}

/// Webhook de um app: a URL e o estado do segredo, nunca o valor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AppWebhook {
    #[schema(example = "https://gestor.interno/engramind/events")]
    pub url: String,
    pub secret: WebhookSecretState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WebhookSecretState {
    pub configured: bool,
    /// Última gravação; `null` sem segredo.
    pub updated_at: Option<String>,
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
    pub webhook_url: Option<String>,
    /// Segredo do webhook já cifrado pelo `Vault`.
    pub webhook_secret: Option<Vec<u8>>,
}

/// Também serve aos agentes (`AgentPatch`), que ignoram os campos do webhook.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppPatch {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub disabled: Option<bool>,
    /// `Some(None)` remove a URL.
    pub webhook_url: Option<Option<String>>,
    /// Segredo cifrado; `Some(None)` remove.
    pub webhook_secret: Option<Option<Vec<u8>>>,
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
    /// Eventos que a mutação dispara se mudar algo de fato (008 D-001); o `Store` cria um job
    /// `webhook` por destinatário na mesma transação.
    pub events: Vec<crate::core::events::EventDraft>,
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

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Off => "off",
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
    /// `create`, `update`, `archive`, `restore`, `pin`, `unpin`, `move`, `revert` ou `redact`
    /// (texto revisado pelo Redator, autor `system/redator`).
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

/// Memória como o contexto a usa (só os campos da ordem e do bloco).
#[derive(Debug, Clone, PartialEq)]
pub struct ContextMemory {
    pub id: String,
    pub channel_id: String,
    pub category: String,
    pub title: String,
    pub text: String,
    pub status: MemoryStatus,
    pub importance: Option<f64>,
    pub created_at: String,
}

/// Memórias `active`/`permanent` e versões dos canais lidas na mesma transação de leitura.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextSnapshot {
    /// `(channel_id, channels.version)` dos canais que ainda existem.
    pub versions: Vec<(String, i64)>,
    pub memories: Vec<ContextMemory>,
}

/// Injeções de uma memória a somar em lote (`injected_count`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectionBump {
    pub memory_id: String,
    pub count: i64,
}

/// Uma linha de `usage_log` (desenho §6): `get_context` entregue ou `search_memories`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEntry {
    /// Instante RFC 3339 em UTC com milissegundos (ordenável como texto).
    pub at: String,
    pub app_id: String,
    pub agent_id: Option<String>,
    pub token_id: String,
    pub op: &'static str,
    pub channel_ids: Vec<String>,
    pub memory_ids: Vec<String>,
    pub rendered_tokens: Option<u32>,
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

// ---------- 006: fila, segredos, chamadas ao LLM e Redator ----------

/// Estado de um job da fila.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Um job da fila `jobs`. O payload só tem ids e versões, nunca texto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Job {
    #[schema(example = 42)]
    pub id: i64,
    /// `redact` (006) ou `embed` (processado a partir da 007).
    #[schema(example = "redact")]
    pub kind: String,
    #[schema(value_type = Object, example = json!({ "memory_id": "m_8f2a1c3d", "version": 1 }))]
    pub payload: serde_json::Value,
    pub state: JobState,
    /// Tentativas iniciadas (cada captura conta uma).
    #[schema(example = 1)]
    pub attempts: i64,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub run_after: String,
    /// Fim do lease de quem está executando; vencido, o job volta a ser capturável.
    pub locked_until: Option<String>,
    /// Motivo curto da última falha (sem texto de memória nem segredo).
    pub last_error: Option<String>,
    /// Em `done`: `applied`, `title_only`, `superseded` ou `skipped`.
    pub outcome: Option<String>,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// Como um job capturado termina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobFinish {
    Done {
        outcome: &'static str,
    },
    /// Volta para `queued` em `run_after`.
    Retry {
        error: String,
        run_after: String,
    },
    Fail {
        error: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobFilter {
    pub state: Option<JobState>,
    pub kind: Option<String>,
}

/// Jobs por tipo e estado (`get_status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct JobCount {
    #[schema(example = "redact")]
    pub kind: String,
    pub state: JobState,
    #[schema(example = 3)]
    pub count: u64,
}

/// Segredo visto de fora: nunca o valor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SecretView {
    #[schema(example = "redator.api_key")]
    pub key: String,
    pub configured: bool,
    /// Última gravação; `null` quando não configurado.
    pub updated_at: Option<String>,
}

/// Segredo cifrado como está no banco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSecret {
    pub value_enc: Vec<u8>,
    pub updated_at: String,
}

/// Uma chamada ao LLM a registrar (sem texto, prompt ou chave).
#[derive(Debug, Clone, PartialEq)]
pub struct NewLlmCall {
    pub at: String,
    pub purpose: &'static str,
    pub provider: String,
    pub model: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
    pub duration_ms: i64,
    pub outcome: &'static str,
    pub error: Option<String>,
}

/// Linha de `llm_calls`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct LlmCall {
    #[schema(example = 7)]
    pub id: i64,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub at: String,
    /// `redact` ou `test`.
    #[schema(example = "redact")]
    pub purpose: String,
    #[schema(example = "openai_compatible")]
    pub provider: String,
    #[schema(example = "gpt-4o-mini")]
    pub model: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    /// Só quando o provedor informa o custo (por exemplo `usage.cost`).
    pub cost_usd: Option<f64>,
    #[schema(example = 820)]
    pub duration_ms: i64,
    /// `success`, `timeout`, `rate_limited`, `unavailable`, `rejected` ou `invalid_response`.
    #[schema(example = "success")]
    pub outcome: String,
    /// Motivo curto da falha.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LlmCallFilter {
    pub since: Option<String>,
    pub purpose: Option<String>,
}

/// Últimos resultados do Redator (`get_status`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LlmActivity {
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
}

/// Resultado do Redator a aplicar numa memória. `expected_hash` é o `content_hash` lido no
/// início do job: se o conteúdo mudou desde então, o `Store` falha com
/// `Conflict("MEMORY_CHANGED")`.
#[derive(Debug, Clone, PartialEq)]
pub struct Redaction {
    pub memory_id: String,
    pub expected_hash: Vec<u8>,
    pub title: String,
    /// Texto revisado e o `content_hash` dele: nova revisão `system/redator`.
    pub text: Option<(String, Vec<u8>)>,
    /// Embeddings ligados: o título (e o texto) novo reenfileira o job `embed`; desligados,
    /// o texto novo fica com `embedding_state = off`.
    pub embed: bool,
}

// ---------- embeddings (007) ----------

/// Chave de `schema_meta` com o `model_id` vigente (vazia com `embedding.provider = off`).
pub const META_EMBEDDING_MODEL: &str = "embedding.model_id";
/// Chave de `schema_meta` com a reindexação em andamento (`ReindexState` em JSON).
pub const META_EMBEDDING_REINDEX: &str = "embedding.reindex";

/// Texto a vetorizar de uma memória.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedSource {
    pub id: String,
    pub title: String,
    pub text: String,
    pub content_hash: Vec<u8>,
}

/// Vetor de uma memória: só é gravado se ela ainda tiver o conteúdo e o título lidos.
#[derive(Debug, Clone, PartialEq)]
pub struct NewVector {
    pub memory_id: String,
    pub content_hash: Vec<u8>,
    pub title: String,
    pub embedding: Vec<f32>,
}

/// Vetores de um mesmo modelo; nada é gravado se `model_id` deixou de ser o vigente.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorBatch {
    pub model_id: String,
    pub dims: usize,
    pub vectors: Vec<NewVector>,
    pub at: String,
}

/// Ramo vetorial da busca: só vetores do `model_id` e da dimensão da consulta.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VectorQuery {
    pub embedding: Vec<f32>,
    pub model_id: String,
    pub channel_ids: Vec<String>,
    pub categories: Vec<String>,
    pub statuses: Vec<MemoryStatus>,
    pub exclude_ids: Vec<String>,
    pub limit: u32,
}

/// Memória do ramo vetorial com a distância de cosseno (`1 - similaridade`).
#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    pub memory: Memory,
    pub distance: f64,
}

/// Reindexação a enfileirar: `all` revetoriza tudo; senão, só o que não tem vetor do modelo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexPlan {
    pub all: bool,
    pub batch_size: usize,
    pub started_at: String,
}

/// Troca do espaço dos vetores na mesma transação das configurações: `model_id = None`
/// desliga (`embedding_state = off`); com `reindex`, marca `pending` e enfileira os lotes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingSwitch {
    pub model_id: Option<String>,
    pub reindex: Option<ReindexPlan>,
}

/// Reindexação em andamento (`schema_meta`), também a resposta de `reembed_all`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ReindexState {
    #[schema(example = "local:multilingual-e5-small")]
    pub model_id: String,
    /// Marca dos jobs desta reindexação (`payload.reindex`).
    #[schema(example = "2026-10-31T12:00:00.000Z")]
    pub started_at: String,
    /// Memórias enfileiradas.
    #[schema(example = 1234)]
    pub total: u64,
}

/// Jobs de uma reindexação.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReindexJobs {
    pub pending: u64,
    pub failed: u64,
}

/// Contagens de vetores para `get_status`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VectorCounts {
    /// Memórias existentes.
    pub memories: u64,
    /// Memórias com vetor do modelo.
    pub vectors: u64,
    /// Desses, os gravados a partir de `since`.
    pub since: u64,
}

// ---------- integração (008) ----------

/// Quem pode receber eventos: apps, agentes e tokens `admin` (cache da geração da rede).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebhookDirectory {
    pub apps: Vec<DirectoryApp>,
    pub agents: Vec<DirectoryAgent>,
    /// Tokens `admin` não revogados: `(app_id, expires_at)`.
    pub admin_tokens: Vec<(String, Option<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryApp {
    pub id: String,
    /// URL e segredo configurados.
    pub webhook: bool,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryAgent {
    pub id: String,
    pub app_id: String,
    pub disabled: bool,
}

/// O que a entrega precisa de um app; lido na hora de cada tentativa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookTarget {
    pub slug: String,
    pub disabled: bool,
    pub url: Option<String>,
    pub secret_enc: Option<Vec<u8>>,
}

/// Memória importada (008 FR-009): autor, data e origem preservados.
#[derive(Debug, Clone, PartialEq)]
pub struct NewImportedMemory {
    /// Posição no lote, devolvida em `created`/`duplicates`.
    pub index: usize,
    pub category: String,
    pub title: String,
    pub text: String,
    pub status: MemoryStatus,
    pub content_hash: Vec<u8>,
    pub origin: Option<Origin>,
    pub created_at: String,
    pub author_app_id: String,
    pub author_agent_id: Option<String>,
    pub jobs: Vec<NewJob>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImportCreated {
    #[schema(example = 0)]
    pub index: usize,
    #[schema(example = "m_8f2a1c3d")]
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImportDuplicate {
    #[schema(example = 3)]
    pub index: usize,
    /// Memória do canal (ou item anterior do lote) com o mesmo texto.
    #[schema(example = "m_1b7d0e44")]
    pub existing_id: String,
}

/// Resultado de `import_memories`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImportResult {
    pub created: Vec<ImportCreated>,
    pub duplicates: Vec<ImportDuplicate>,
}

/// Agrupamento de `get_usage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageGroup {
    Day,
    Agent,
    Operation,
    Channel,
}

impl UsageGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Agent => "agent",
            Self::Operation => "operation",
            Self::Channel => "channel",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageQuery {
    pub since: String,
    pub until: String,
    pub group: UsageGroup,
}

/// Uma linha agregada de `usage_log`. `key` é o dia, o id do agente (ou do app, para tokens
/// sem agente), a operação ou o id do canal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageAggregate {
    pub key: String,
    pub requests: u64,
    pub memories: u64,
    pub rendered_tokens: u64,
}

/// Chamadas do LLM de uma finalidade num período.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct LlmSummary {
    #[schema(example = 40)]
    pub calls: u64,
    #[schema(example = 1)]
    pub failures: u64,
    /// Soma de `cost_usd` das chamadas com custo conhecido.
    #[schema(example = 0.0123)]
    pub cost_usd: f64,
}

/// Jobs `webhook` pendentes e terminados desde um instante.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WebhookCounts {
    pub pending: u64,
    pub delivered: u64,
    pub failed: u64,
}

/// Tabelas com retenção (008 FR-014); um enum fechado, o `Store` monta o SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionTable {
    UsageLog,
    LlmCalls,
    AuditLog,
    Jobs,
    Idempotency,
}

impl RetentionTable {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UsageLog => "usage_log",
            Self::LlmCalls => "llm_calls",
            Self::AuditLog => "audit_log",
            Self::Jobs => "jobs",
            Self::Idempotency => "idempotency",
        }
    }
}
