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
        IdemRow, IdentityCounts, Lookup, NewAgent, NewApp, NewToken, SettingChange, SettingRow,
        Token, TokenFilter, TokenPatch, WriteContext,
    },
    pagination::{Page, PageRequest},
};

pub use sqlite::SqliteStore;

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
    /// Remove o app com agentes e tokens (cascata).
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
    /// Remove o agente com seus tokens (cascata).
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

    // Configurações, idempotência e auditoria.

    fn get_settings(&self) -> impl Future<Output = Result<Vec<SettingRow>, StoreError>> + Send;
    fn update_settings(
        &self,
        changes: Vec<SettingChange>,
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
}
