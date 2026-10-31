//! Armazenamento atrás da interface `Store`. Nenhum módulo fora de `store::sqlite`
//! conhece SQL ou tipos do `rusqlite`.

pub mod sqlite;

use std::future::Future;

use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

use crate::core::{
    model::{
        Agent, AgentLookup, AgentPatch, App, AppPatch, AuditEvent, AuditFilter, AuthTokenRow,
        Channel, ChannelPatch, ChannelStats, ContextSnapshot, EmbedSource, EmbeddingState,
        EmbeddingSwitch, IdemRow, IdentityCounts, InjectionBump, Job, JobCount, JobFilter,
        JobFinish, LlmActivity, LlmCall, LlmCallFilter, Lookup, Memory, MemoryFilter, MemoryPatch,
        MemoryRevision, MemoryTarget, MemoryWrite, Network, NewAgent, NewApp, NewChannel,
        NewLlmCall, NewMemory, NewSubscription, NewToken, Redaction, ReindexJobs, ReindexPlan,
        ReindexState, RetrievalBump, SecretView, SettingChange, SettingRow, StatusAction,
        StoredSecret, Subscription, SubscriptionPatch, TextQuery, Token, TokenFilter, TokenPatch,
        UsageEntry, VectorBatch, VectorCounts, VectorHit, VectorQuery, WriteContext,
    },
    pagination::{Page, PageRequest},
};

pub use sqlite::SqliteStore;

/// Sobreposições de configuração de um canal: `chave → valor`.
pub type ChannelSettingsMap = std::collections::BTreeMap<String, serde_json::Value>;

/// Implementação usada pelo `AppState` (D-004). Trocar de banco altera só este alias.
pub type AppStore = SqliteStore;

pub trait Store: Clone + Send + Sync + 'static {
    /// Banco acessível e estado das migrações.
    fn ready(&self) -> impl Future<Output = Result<ReadyReport, StoreError>> + Send;
    /// `integrity_check`, migrações e extensões.
    fn diagnose(&self) -> impl Future<Output = Result<DoctorReport, StoreError>> + Send;
    fn meta(&self, key: &str) -> impl Future<Output = Result<Option<Vec<u8>>, StoreError>> + Send;
    fn set_meta(
        &self,
        key: &str,
        value: &[u8],
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    // Identidades. Toda mutação grava a auditoria e a idempotência de `WriteContext` na mesma
    // transação. Mudanças que deixariam a rede sem token admin ativo com `write` falham com
    // `Conflict("LAST_ADMIN_TOKEN")`; slug repetido, com `Conflict("SLUG_ALREADY_EXISTS")`.

    /// Token com digest e estado de app/agente, para a autenticação.
    fn find_auth_token(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<AuthTokenRow>, StoreError>> + Send;
    fn count_tokens(&self) -> impl Future<Output = Result<u64, StoreError>> + Send;
    fn identity_counts(&self) -> impl Future<Output = Result<IdentityCounts, StoreError>> + Send;
    fn list_apps(
        &self,
        page: PageRequest,
    ) -> impl Future<Output = Result<Page<App>, StoreError>> + Send;
    fn get_app(
        &self,
        lookup: Lookup,
    ) -> impl Future<Output = Result<Option<App>, StoreError>> + Send;
    fn create_app(
        &self,
        new: NewApp,
        context: WriteContext,
    ) -> impl Future<Output = Result<App, StoreError>> + Send;
    fn update_app(
        &self,
        id: String,
        patch: AppPatch,
        context: WriteContext,
    ) -> impl Future<Output = Result<App, StoreError>> + Send;
    /// Remove o app com agentes, tokens e assinaturas (cascata).
    fn delete_app(
        &self,
        id: String,
        context: WriteContext,
    ) -> impl Future<Output = Result<App, StoreError>> + Send;
    fn list_agents(
        &self,
        app_id: Option<String>,
        page: PageRequest,
    ) -> impl Future<Output = Result<Page<Agent>, StoreError>> + Send;
    fn get_agent(
        &self,
        lookup: AgentLookup,
    ) -> impl Future<Output = Result<Option<Agent>, StoreError>> + Send;
    fn create_agent(
        &self,
        new: NewAgent,
        context: WriteContext,
    ) -> impl Future<Output = Result<Agent, StoreError>> + Send;
    fn update_agent(
        &self,
        id: String,
        patch: AgentPatch,
        context: WriteContext,
    ) -> impl Future<Output = Result<Agent, StoreError>> + Send;
    /// Remove o agente com seus tokens e assinaturas (cascata).
    fn delete_agent(
        &self,
        id: String,
        context: WriteContext,
    ) -> impl Future<Output = Result<Agent, StoreError>> + Send;
    fn list_tokens(
        &self,
        filter: TokenFilter,
        page: PageRequest,
    ) -> impl Future<Output = Result<Page<Token>, StoreError>> + Send;
    fn get_token(
        &self,
        id: String,
    ) -> impl Future<Output = Result<Option<Token>, StoreError>> + Send;
    fn create_token(
        &self,
        new: NewToken,
        context: WriteContext,
    ) -> impl Future<Output = Result<Token, StoreError>> + Send;
    fn update_token(
        &self,
        id: String,
        patch: TokenPatch,
        context: WriteContext,
    ) -> impl Future<Output = Result<Token, StoreError>> + Send;
    fn revoke_token(
        &self,
        id: String,
        context: WriteContext,
    ) -> impl Future<Output = Result<Token, StoreError>> + Send;
    /// Cria o app (ou reaproveita o slug) e o token do bootstrap só se não existir nenhum
    /// token; devolve `None` quando já havia tokens.
    fn bootstrap(
        &self,
        app: NewApp,
        token: NewToken,
        context: WriteContext,
    ) -> impl Future<Output = Result<Option<Token>, StoreError>> + Send;
    /// Grava `last_used_at` em lote: `(token_id, instante)`.
    fn touch_tokens(
        &self,
        batch: Vec<(String, String)>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    // Canais e assinaturas. Mutações gravam auditoria e idempotência na mesma transação; toda
    // mudança de canal incrementa `channels.version`.

    /// Todos os canais (com os arquivados, por slug) e todas as assinaturas (por criação), com
    /// `reaches` calculado.
    fn network(&self) -> impl Future<Output = Result<Network, StoreError>> + Send;
    fn get_channel(
        &self,
        lookup: Lookup,
    ) -> impl Future<Output = Result<Option<Channel>, StoreError>> + Send;
    /// Slug repetido → `Conflict("SLUG_ALREADY_EXISTS")`.
    fn create_channel(
        &self,
        new: NewChannel,
        context: WriteContext,
    ) -> impl Future<Output = Result<Channel, StoreError>> + Send;
    fn update_channel(
        &self,
        id: String,
        patch: ChannelPatch,
        context: WriteContext,
    ) -> impl Future<Output = Result<Channel, StoreError>> + Send;
    /// Arquiva (`true`) ou restaura (`false`); sem mudança de estado, devolve o canal intacto.
    fn set_channel_archived(
        &self,
        id: String,
        archived: bool,
        context: WriteContext,
    ) -> impl Future<Output = Result<Channel, StoreError>> + Send;
    /// Só canal arquivado (`Conflict("CHANNEL_NOT_ARCHIVED")`); remove as assinaturas exatas.
    fn delete_channel(
        &self,
        id: String,
        context: WriteContext,
    ) -> impl Future<Output = Result<Channel, StoreError>> + Send;
    /// Alvo exato inexistente → `Conflict("CHANNEL_NOT_FOUND")`; repetida →
    /// `Conflict("SUBSCRIPTION_EXISTS")`.
    fn create_subscription(
        &self,
        new: NewSubscription,
        context: WriteContext,
    ) -> impl Future<Output = Result<Subscription, StoreError>> + Send;
    fn update_subscription(
        &self,
        id: String,
        patch: SubscriptionPatch,
        context: WriteContext,
    ) -> impl Future<Output = Result<Subscription, StoreError>> + Send;
    fn delete_subscription(
        &self,
        id: String,
        context: WriteContext,
    ) -> impl Future<Output = Result<Subscription, StoreError>> + Send;

    // Configurações, idempotência e auditoria.

    /// Aplica as sobreposições do canal (`None` remove) e devolve as resultantes; incrementa
    /// `channels.version`.
    fn update_channel_settings(
        &self,
        channel_id: String,
        changes: Vec<SettingChange>,
        context: WriteContext,
    ) -> impl Future<Output = Result<ChannelSettingsMap, StoreError>> + Send;

    // Memórias. Toda mutação grava memória, revisão, auditoria (sem texto), `memories.version`,
    // `channels.version`, jobs e idempotência na mesma transação. Canal arquivado →
    // `Conflict("CHANNEL_ARCHIVED")`; memória fora do canal de `MemoryTarget` → `NotFound`.

    fn get_memory(
        &self,
        id: String,
    ) -> impl Future<Output = Result<Option<Memory>, StoreError>> + Send;
    /// Id de outra memória do canal com o mesmo `content_hash`.
    fn find_memory_by_hash(
        &self,
        channel_id: String,
        content_hash: Vec<u8>,
        exclude: Option<String>,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;
    /// Por `updated_at` decrescente.
    fn list_memories(
        &self,
        filter: MemoryFilter,
        page: PageRequest,
    ) -> impl Future<Output = Result<Page<Memory>, StoreError>> + Send;
    /// Da versão mais nova à mais antiga.
    fn memory_history(
        &self,
        id: String,
        page: PageRequest,
    ) -> impl Future<Output = Result<Page<MemoryRevision>, StoreError>> + Send;
    fn get_revision(
        &self,
        id: String,
        version: i64,
    ) -> impl Future<Output = Result<Option<MemoryRevision>, StoreError>> + Send;
    /// Busca textual por relevância (BM25 sem acentos no SQLite).
    fn search_text(
        &self,
        query: TextQuery,
    ) -> impl Future<Output = Result<Vec<Memory>, StoreError>> + Send;
    /// Com `dedupe`, texto igual no canal devolve a existente com `duplicate: true` e não grava
    /// a memória (a idempotência fica com status 200).
    fn create_memory(
        &self,
        new: NewMemory,
        context: WriteContext,
    ) -> impl Future<Output = Result<MemoryWrite, StoreError>> + Send;
    /// `Conflict("MEMORY_VERSION_CONFLICT")` com `expected_version` divergente;
    /// `Conflict("MEMORY_DUPLICATE")` quando o texto novo repete outra memória do canal.
    fn update_memory(
        &self,
        target: MemoryTarget,
        patch: MemoryPatch,
        context: WriteContext,
    ) -> impl Future<Output = Result<MemoryWrite, StoreError>> + Send;
    /// Sem mudança de estado, devolve a memória intacta.
    fn set_memory_status(
        &self,
        target: MemoryTarget,
        action: StatusAction,
        context: WriteContext,
    ) -> impl Future<Output = Result<Memory, StoreError>> + Send;
    /// Incrementa a versão dos dois canais; duplicata no destino →
    /// `Conflict("MEMORY_DUPLICATE")`.
    fn move_memory(
        &self,
        target: MemoryTarget,
        to_channel_id: String,
        dedupe: bool,
        context: WriteContext,
    ) -> impl Future<Output = Result<Memory, StoreError>> + Send;
    /// Mantém só a última revisão.
    fn delete_memory(
        &self,
        target: MemoryTarget,
        context: WriteContext,
    ) -> impl Future<Output = Result<Memory, StoreError>> + Send;
    /// Soma `retrieval_count` e avança `last_retrieved_at` sem mudar `version`.
    fn bump_retrievals(
        &self,
        batch: Vec<RetrievalBump>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
    fn channel_stats(
        &self,
        channel_id: String,
    ) -> impl Future<Output = Result<Option<ChannelStats>, StoreError>> + Send;
    /// Memórias `active`/`permanent` dos canais e as versões deles, no mesmo instantâneo.
    fn context_memories(
        &self,
        channel_ids: Vec<String>,
    ) -> impl Future<Output = Result<ContextSnapshot, StoreError>> + Send;
    /// Soma `injected_count` sem mudar `version`.
    fn bump_injections(
        &self,
        batch: Vec<InjectionBump>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
    /// Grava as linhas acumuladas de `usage_log`.
    fn append_usage(
        &self,
        batch: Vec<UsageEntry>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
    /// Memórias por categoria em toda a rede.
    fn category_counts(
        &self,
    ) -> impl Future<Output = Result<Vec<(String, u64)>, StoreError>> + Send;

    fn get_settings(&self) -> impl Future<Output = Result<Vec<SettingRow>, StoreError>> + Send;
    /// Com `retained_categories`, falha com `Conflict("CATEGORY_IN_USE")` se alguma memória
    /// usa uma categoria fora da lista.
    /// Com `embedding`, troca o espaço dos vetores na mesma transação (`model_id` vigente,
    /// estados, lotes `embed` e o registro da reindexação).
    fn update_settings(
        &self,
        changes: Vec<SettingChange>,
        retained_categories: Option<Vec<String>>,
        embedding: Option<EmbeddingSwitch>,
        context: WriteContext,
    ) -> impl Future<Output = Result<Vec<SettingRow>, StoreError>> + Send;
    fn find_idempotency(
        &self,
        token_id: &str,
        key: &str,
    ) -> impl Future<Output = Result<Option<IdemRow>, StoreError>> + Send;
    /// Remove registros de idempotência criados antes de `before`; devolve quantos.
    fn purge_idempotency(
        &self,
        before: String,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;
    fn list_audit(
        &self,
        filter: AuditFilter,
        page: PageRequest,
    ) -> impl Future<Output = Result<Page<AuditEvent>, StoreError>> + Send;

    // Fila de jobs (006). A captura é um `UPDATE … RETURNING` que pega o job pronto mais
    // antigo do tipo (ou um `running` com lease vencido) e conta a tentativa.

    fn claim_job(
        &self,
        kind: String,
        now: String,
        lease_until: String,
    ) -> impl Future<Output = Result<Option<Job>, StoreError>> + Send;
    /// Só termina jobs `running`; devolve `false` se o job não estava mais com o executor.
    fn finish_job(
        &self,
        id: i64,
        finish: JobFinish,
        at: String,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
    /// Existe job do mesmo tipo, mais novo que `after_id`, para a memória.
    fn newer_job_exists(
        &self,
        kind: String,
        memory_id: String,
        after_id: i64,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
    /// Do mais recente ao mais antigo.
    fn list_jobs(
        &self,
        filter: JobFilter,
        page: PageRequest,
    ) -> impl Future<Output = Result<Page<Job>, StoreError>> + Send;
    /// Recoloca um job `failed` na fila; outro estado → `Conflict("JOB_NOT_RETRYABLE")`.
    fn retry_job(
        &self,
        id: i64,
        context: WriteContext,
    ) -> impl Future<Output = Result<Job, StoreError>> + Send;
    fn job_counts(&self) -> impl Future<Output = Result<Vec<JobCount>, StoreError>> + Send;

    // Segredos cifrados (006): o `Store` só vê bytes cifrados.

    fn get_secret(
        &self,
        key: String,
    ) -> impl Future<Output = Result<Option<StoredSecret>, StoreError>> + Send;
    fn list_secrets(&self) -> impl Future<Output = Result<Vec<SecretView>, StoreError>> + Send;
    /// `None` apaga. Grava auditoria (sem o valor) e idempotência.
    fn set_secret(
        &self,
        key: String,
        value_enc: Option<Vec<u8>>,
        context: WriteContext,
    ) -> impl Future<Output = Result<SecretView, StoreError>> + Send;

    // Chamadas ao LLM e Redator (006).

    /// Grava as chamadas e, com `remember`, a resposta idempotente na mesma transação.
    fn append_llm_calls(
        &self,
        calls: Vec<NewLlmCall>,
        remember: Option<(WriteContext, serde_json::Value)>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
    /// Da mais recente à mais antiga.
    fn list_llm_calls(
        &self,
        filter: LlmCallFilter,
        page: PageRequest,
    ) -> impl Future<Output = Result<Page<LlmCall>, StoreError>> + Send;
    fn llm_activity(
        &self,
        purpose: String,
    ) -> impl Future<Output = Result<LlmActivity, StoreError>> + Send;
    /// Aplica título (e texto) do Redator: `title_state = ready`, `channels.version + 1` e
    /// auditoria; com texto, também revisão, `memories.version + 1` e job `embed`. Memória
    /// inexistente → `NotFound`; conteúdo mudado → `Conflict("MEMORY_CHANGED")`.
    fn apply_redaction(
        &self,
        redaction: Redaction,
        context: WriteContext,
    ) -> impl Future<Output = Result<Memory, StoreError>> + Send;
    /// `title_state = fallback` se o conteúdo ainda for o de `expected_hash` e o título ainda
    /// estiver `pending`; devolve se mudou.
    fn set_title_fallback(
        &self,
        memory_id: String,
        expected_hash: Vec<u8>,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
    /// `content_hash` atual da memória.
    fn memory_content_hash(
        &self,
        memory_id: String,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, StoreError>> + Send;

    // Embeddings (007). O `model_id` vigente fica no próprio `Store` (gravado com as
    // configurações), para um vetor de outro modelo nunca entrar depois de uma troca.

    /// `model_id` vigente; `Some("")` com os embeddings desligados, `None` antes da 0.7.0.
    fn embedding_model(&self) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;
    /// Grava o `model_id` vigente sem reindexar (inicialização).
    fn set_embedding_model(
        &self,
        model_id: String,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
    /// Título, texto e `content_hash` das memórias que ainda existem.
    fn embed_sources(
        &self,
        ids: Vec<String>,
    ) -> impl Future<Output = Result<Vec<EmbedSource>, StoreError>> + Send;
    /// Grava os vetores ainda válidos (`embedding_state = ready`); devolve quantos.
    fn save_vectors(
        &self,
        batch: VectorBatch,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;
    /// `embedding_state` (`failed` ou `off`) das memórias ainda `pending`.
    fn mark_embedding(
        &self,
        ids: Vec<String>,
        state: EmbeddingState,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;
    /// Ramo vetorial da busca, da menor para a maior distância.
    fn search_vectors(
        &self,
        query: VectorQuery,
    ) -> impl Future<Output = Result<Vec<VectorHit>, StoreError>> + Send;
    fn vector_counts(
        &self,
        model_id: String,
        since: Option<String>,
    ) -> impl Future<Output = Result<VectorCounts, StoreError>> + Send;
    fn reindex_state(
        &self,
    ) -> impl Future<Output = Result<Option<ReindexState>, StoreError>> + Send;
    /// Jobs pendentes e falhos da reindexação, fora `exclude_job`.
    fn reindex_jobs(
        &self,
        started_at: String,
        exclude_job: Option<i64>,
    ) -> impl Future<Output = Result<ReindexJobs, StoreError>> + Send;
    /// `reembed_all`: revetoriza tudo com `model_id`, com auditoria e idempotência;
    /// reindexação com jobs pendentes → `Conflict("REEMBED_IN_PROGRESS")`.
    fn start_reindex(
        &self,
        model_id: String,
        plan: ReindexPlan,
        context: WriteContext,
    ) -> impl Future<Output = Result<ReindexState, StoreError>> + Send;
    /// Fim da reindexação `started_at` (ou limpeza sem reindexação, com `None`): apaga os
    /// vetores de outros modelos e o registro; devolve se encerrou.
    fn finish_reindex(
        &self,
        model_id: String,
        started_at: Option<String>,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Escritor parado, pool esgotado por timeout ou falha de I/O.
    #[error("banco de dados indisponivel")]
    Unavailable,
    #[error("conflito: {0}")]
    Conflict(&'static str),
    #[error("registro nao encontrado")]
    NotFound,
    /// Nunca contém dados de memória.
    #[error("falha interna do banco: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, ToSchema)]
pub struct MigrationStatus {
    /// Migrações aplicadas no banco.
    #[schema(example = 1)]
    pub applied: u32,
    /// Migrações embutidas no binário.
    #[schema(example = 1)]
    pub expected: u32,
}

impl MigrationStatus {
    pub fn is_current(self) -> bool {
        self.applied == self.expected
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyReport {
    pub migrations: MigrationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    /// Resultado de `PRAGMA integrity_check` (`ok` quando íntegro).
    pub integrity: String,
    pub migrations: MigrationStatus,
    pub sqlite_version: String,
    /// Versão do `sqlite-vec`, ou `None` quando a extensão não carregou.
    pub vec_version: Option<String>,
    pub fts5: bool,
    pub journal_mode: String,
    /// `integrity-check` do `memory_fts` contra `memories` (`ok` quando sincronizado), ou
    /// `None` antes da migração das memórias.
    pub memory_fts: Option<String>,
}
