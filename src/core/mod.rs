//! Camada de domínio. REST, MCP e CLI são adaptadores finos sobre ela: toda operação passa por
//! `Core`, com a mesma autorização, validação, auditoria, idempotência e códigos de erro.

pub mod audit;
pub mod auth;
pub mod authz;
pub mod background;
pub mod bootstrap;
pub mod docs;
pub mod idempotency;
pub mod identities;
pub mod ids;
pub mod model;
pub mod operations;
pub mod pagination;
pub mod pepper;
pub mod principal;
pub mod settings;
pub mod status;
pub mod usage;

use std::sync::{Arc, RwLock};

use crate::{
    auth::token::TokenService,
    config::{DatabaseSettings, Secret},
    error::ApiError,
    store::{Store, StoreError},
};

use self::{
    docs::Docs, model::Actor, operations::OperationSpec, principal::Principal,
    settings::SettingsSnapshot, usage::Usage,
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
    database: DatabaseSettings,
}

impl<S: Store> Core<S> {
    /// Monta o `core` com as configurações padrão; `reload_settings` lê as persistidas.
    pub fn new(store: S, pepper: Secret, database: DatabaseSettings) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                tokens: TokenService::new(pepper),
                docs: Docs::new(crate::openapi::document()),
                settings: RwLock::new(SettingsSnapshot::default()),
                usage: Usage::default(),
                database,
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
        other => ApiError::from(other),
    }
}
