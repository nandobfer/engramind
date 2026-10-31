//! Camada de domínio. REST, MCP e CLI são adaptadores finos sobre ela: toda operação passa por
//! `Core`, com a mesma autorização, validação, auditoria, idempotência e códigos de erro.

pub mod access;
pub mod audit;
pub mod auth;
pub mod authz;
pub mod background;
pub mod bootstrap;
pub mod channels;
pub mod context;
pub mod counters;
pub mod docs;
pub mod embeddings;
pub mod events;
pub mod hints;
pub mod idempotency;
pub mod identities;
pub mod ids;
pub mod import;
pub mod jobs;
pub mod memories;
pub mod memory_text;
pub mod model;
pub mod operations;
pub mod pagination;
pub mod pepper;
pub mod principal;
pub mod redator;
pub mod retention;
pub mod search;
pub mod secrets;
pub mod selection;
pub mod settings;
pub mod status;
pub mod subscriptions;
pub mod usage;
pub mod usage_report;
pub mod webhooks;

use std::{
    sync::{Arc, RwLock},
    time::Instant,
};

use tokio::sync::Notify;

use crate::{
    auth::token::TokenService,
    config::{DatabaseSettings, ModelsSettings, Secret, SecretKey},
    error::ApiError,
    providers::embeddings::{Embedder, LocalEmbedder},
    store::{Store, StoreError},
};

use self::{
    access::NetworkCache, context::cache::ContextCache, counters::Counters, docs::Docs,
    events::DirectoryCache, model::Actor, operations::OperationSpec, principal::Principal,
    redator::guard::MeaningGuard, secrets::Vault, settings::SettingsSnapshot, status::BackupSample,
    usage::Usage,
};

/// Ponto de entrada das operações. Barato de clonar.
#[derive(Clone)]
pub struct Core<S: Store> {
    inner: Arc<Inner<S>>,
}

struct Inner<S> {
    store: S,
    tokens: TokenService,
    docs: Docs,
    settings: RwLock<SettingsSnapshot>,
    usage: Usage,
    counters: Counters,
    network: NetworkCache,
    context: ContextCache,
    database: DatabaseSettings,
    vault: Vault,
    http: reqwest::Client,
    /// Acorda os runners da fila depois de um commit que enfileirou jobs.
    jobs: Arc<Notify>,
    /// Guarda de sentido trocada pelos testes; sem ela, a dos embeddings vigentes.
    guard: RwLock<Option<Arc<dyn MeaningGuard>>>,
    /// Provedor `local` (fastembed) compartilhado pelo processo.
    local: LocalEmbedder,
    /// Substituto do provedor `local` nos testes (embeddings por hash).
    local_override: RwLock<Option<Arc<dyn Embedder>>>,
    /// Destinatários dos webhooks, na mesma geração da rede.
    directory: DirectoryCache,
    /// Instante de subida (uptime de `get_status`).
    started: Instant,
    /// Última medição da réplica do Litestream (amostrador de `background`, 008 Q-009).
    backup: RwLock<Option<BackupSample>>,
}

impl<S: Store> Core<S> {
    /// Monta o `core` com as configurações padrão; `reload_settings` lê as persistidas.
    pub fn new(
        store: S,
        pepper: Secret,
        secrets_key: SecretKey,
        database: DatabaseSettings,
        models: ModelsSettings,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                tokens: TokenService::new(pepper),
                docs: Docs::new(crate::openapi::document()),
                settings: RwLock::new(SettingsSnapshot::default()),
                usage: Usage::default(),
                counters: Counters::default(),
                network: NetworkCache::default(),
                context: ContextCache::default(),
                database,
                vault: Vault::new(secrets_key),
                http: crate::providers::llm::http_client(),
                jobs: Arc::new(Notify::new()),
                guard: RwLock::new(None),
                local: LocalEmbedder::new(models.path, models.onnx_runtime),
                local_override: RwLock::new(None),
                directory: DirectoryCache::default(),
                started: Instant::now(),
                backup: RwLock::new(None),
            }),
        }
    }

    /// Carrega as configurações persistidas (depois das migrações).
    pub async fn reload_settings(&self) -> Result<(), StoreError> {
        let rows = self.store().get_settings().await?;
        self.replace_settings(SettingsSnapshot::from_rows(&rows));
        Ok(())
    }

    pub fn store(&self) -> &S {
        &self.inner.store
    }

    pub fn docs(&self) -> &Docs {
        &self.inner.docs
    }

    pub(crate) fn tokens(&self) -> &TokenService {
        &self.inner.tokens
    }

    pub(crate) fn usage(&self) -> &Usage {
        &self.inner.usage
    }

    pub(crate) fn counters(&self) -> &Counters {
        &self.inner.counters
    }

    pub(crate) fn network_cache(&self) -> &NetworkCache {
        &self.inner.network
    }

    pub(crate) fn context_cache(&self) -> &ContextCache {
        &self.inner.context
    }

    pub(crate) fn directory_cache(&self) -> &DirectoryCache {
        &self.inner.directory
    }

    pub(crate) fn started(&self) -> Instant {
        self.inner.started
    }

    pub(crate) fn backup_sample(&self) -> Option<BackupSample> {
        self.inner
            .backup
            .read()
            .ok()
            .and_then(|sample| sample.clone())
    }

    pub(crate) fn set_backup_sample(&self, sample: BackupSample) {
        if let Ok(mut current) = self.inner.backup.write() {
            *current = Some(sample);
        }
    }

    /// Invalida o cache da rede (e o diretório dos webhooks) depois de mudar canais,
    /// assinaturas, apps, agentes ou tokens.
    pub(crate) fn network_changed(&self) {
        self.inner.network.invalidate();
    }

    pub(crate) fn vault(&self) -> &Vault {
        &self.inner.vault
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    pub(crate) fn jobs_notify(&self) -> Arc<Notify> {
        self.inner.jobs.clone()
    }

    /// Acorda os runners: há jobs novos na fila.
    pub(crate) fn jobs_enqueued(&self) {
        self.inner.jobs.notify_waiters();
    }

    /// Troca a guarda de sentido do Redator (testes); tem precedência sobre a dos embeddings.
    pub fn set_meaning_guard(&self, guard: Arc<dyn MeaningGuard>) {
        if let Ok(mut current) = self.inner.guard.write() {
            *current = Some(guard);
        }
    }

    /// Guarda de sentido: a dos testes, senão a dos embeddings vigentes; sem nenhuma, `None`
    /// (falha fechada: só o título).
    pub(crate) async fn meaning_guard(&self) -> Option<Arc<dyn MeaningGuard>> {
        let fixed = self.inner.guard.read().ok().and_then(|guard| guard.clone());
        match fixed {
            Some(guard) => Some(guard),
            None => self.embedding_guard().await,
        }
    }

    /// Substitui o provedor `local` (testes): embeddings determinísticos, sem ONNX Runtime.
    pub fn set_local_embedder(&self, embedder: Arc<dyn Embedder>) {
        if let Ok(mut current) = self.inner.local_override.write() {
            *current = Some(embedder);
        }
    }

    pub(crate) fn local_embedder(&self) -> Arc<dyn Embedder> {
        self.inner
            .local_override
            .read()
            .ok()
            .and_then(|current| current.clone())
            .unwrap_or_else(|| Arc::new(self.inner.local.clone()))
    }

    pub(crate) fn models_path(&self) -> &std::path::Path {
        self.inner.local.models_path()
    }

    pub(crate) fn database(&self) -> &DatabaseSettings {
        &self.inner.database
    }

    pub fn settings_snapshot(&self) -> SettingsSnapshot {
        self.inner
            .settings
            .read()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }

    pub(crate) fn replace_settings(&self, snapshot: SettingsSnapshot) {
        if let Ok(mut current) = self.inner.settings.write() {
            *current = snapshot;
        }
    }

    /// Autoriza a operação e registra o nome dela no span da requisição.
    pub(crate) fn authorize(
        &self,
        principal: &Principal,
        name: &str,
    ) -> Result<&'static OperationSpec, ApiError> {
        let operation = operations::spec(name);
        tracing::Span::current().record("operation", operation.name);
        authz::require(principal, operation)?;
        Ok(operation)
    }
}

pub(crate) fn actor(principal: &Principal) -> Actor {
    Actor {
        app_id: Some(principal.app.id.clone()),
        agent_id: principal.agent.as_ref().map(|agent| agent.id.clone()),
        token_id: Some(principal.token_id.clone()),
    }
}

/// Erros do `Store` nos códigos do contrato; `not_found` cobre `StoreError::NotFound`.
pub(crate) fn store_error(error: StoreError, not_found: impl FnOnce() -> ApiError) -> ApiError {
    match error {
        StoreError::NotFound => not_found(),
        StoreError::Conflict("SLUG_ALREADY_EXISTS") => {
            ApiError::new("SLUG_ALREADY_EXISTS", "Ja existe um registro com esse slug")
        }
        StoreError::Conflict("LAST_ADMIN_TOKEN") => ApiError::new(
            "LAST_ADMIN_TOKEN",
            "A mudanca deixaria a rede sem nenhum token admin ativo com write",
        ),
        StoreError::Conflict("IDEMPOTENCY_KEY_EXISTS") => idempotency::key_taken(),
        StoreError::Conflict("CHANNEL_NOT_FOUND") => channels::channel_not_found(),
        StoreError::Conflict("CHANNEL_NOT_ARCHIVED") => {
            ApiError::new("CHANNEL_NOT_ARCHIVED", "Arquive o canal antes de exclui-lo")
        }
        StoreError::Conflict("CHANNEL_ARCHIVED") => ApiError::new(
            "CHANNEL_ARCHIVED",
            "O canal esta arquivado; restaure-o antes de escrever",
        ),
        StoreError::Conflict("JOB_NOT_RETRYABLE") => jobs::job_not_retryable(),
        StoreError::Conflict("REEMBED_IN_PROGRESS") => ApiError::new(
            "REEMBED_IN_PROGRESS",
            "Ja ha uma reindexacao em andamento; acompanhe em get_status",
        ),
        StoreError::Conflict("SUBSCRIPTION_EXISTS") => {
            ApiError::new("SUBSCRIPTION_EXISTS", "O agente ja assina esse alvo")
        }
        other => ApiError::from(other),
    }
}
