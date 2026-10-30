//! Armazenamento atrás da interface `Store`. Nenhum módulo fora de `store::sqlite`
//! conhece SQL ou tipos do `rusqlite`.

pub mod sqlite;

use std::future::Future;

use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
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
