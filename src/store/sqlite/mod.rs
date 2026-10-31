//! Implementação SQLite do `Store`: uma thread escritora e um pool de leitores (D-005),
//! WAL, FTS5 e `sqlite-vec`.

mod channels;
mod context;
mod identities;
mod journal;
mod memories;
mod migrations;
mod readers;
mod settings;
mod vec_extension;
mod writer;

use std::{fs, path::Path, time::Duration};

use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::Serialize;

use self::{readers::Readers, writer::Writer};
use super::{ChannelSettingsMap, DoctorReport, MigrationStatus, ReadyReport, Store, StoreError};
use crate::core::{
    model::{
        Agent, AgentLookup, AgentPatch, App, AppPatch, AuditEvent, AuditFilter, AuthTokenRow,
        Channel, ChannelPatch, ChannelStats, ContextSnapshot, IdemRow, IdentityCounts,
        InjectionBump, Lookup, Memory, MemoryFilter, MemoryPatch, MemoryRevision, MemoryTarget,
        MemoryWrite, Network, NewAgent, NewApp, NewChannel, NewMemory, NewSubscription, NewToken,
        RetrievalBump, SettingChange, SettingRow, StatusAction, Subscription, SubscriptionPatch,
        TextQuery, Token, TokenFilter, TokenPatch, UsageEntry, WriteContext,
    },
    pagination::{Page, PageRequest},
};

const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

#[derive(Clone)]
pub struct SqliteStore {
    writer: Writer,
    readers: Readers,
}

/// Resultado de `migrate`: quantas migrações existiam antes e quantas existem agora.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MigrationOutcome {
    pub before: u32,
    pub after: u32,
    pub expected: u32,
}

impl MigrationOutcome {
    pub fn newly_applied(self) -> u32 {
        self.after - self.before
    }
}

impl SqliteStore {
    /// Abre (ou cria) o banco: registra o `sqlite-vec`, abre o escritor e `read_pool_size`
    /// leitores somente leitura, com os PRAGMAs em todas as conexões. Não migra.
    pub fn open(path: &Path, read_pool_size: usize) -> Result<Self, StoreError> {
        if !vec_extension::register() {
            return Err(StoreError::Internal(
                "falha ao registrar a extensao sqlite-vec".to_owned(),
            ));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| {
                StoreError::Internal(format!("falha ao criar {}: {error}", parent.display()))
            })?;
        }
        let open_error = |error: rusqlite::Error| {
            StoreError::Internal(format!("falha ao abrir {}: {error}", path.display()))
        };

        let connection = Connection::open(path).map_err(open_error)?;
        configure_writer(&connection).map_err(open_error)?;
        let writer = Writer::spawn(connection)?;

        let mut connections = Vec::with_capacity(read_pool_size.max(1));
        for _ in 0..read_pool_size.max(1) {
            let connection = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | OpenFlags::SQLITE_OPEN_URI,
            )
            .map_err(open_error)?;
            configure_reader(&connection).map_err(open_error)?;
            connections.push(connection);
        }

        Ok(Self {
            writer,
            readers: Readers::new(connections),
        })
    }

    /// Aplica as migrações pendentes na thread escritora.
    pub async fn migrate(&self) -> Result<MigrationOutcome, StoreError> {
        self.writer
            .run(|connection| {
                let before = migrations::applied(connection).map_err(sql_error)?;
                migrations::to_latest(connection)
                    .map_err(|error| StoreError::Internal(format!("migracao: {error}")))?;
                let after = migrations::applied(connection).map_err(sql_error)?;
                Ok(MigrationOutcome {
                    before,
                    after,
                    expected: migrations::expected(),
                })
            })
            .await
    }

    /// Escrita serializada numa transação `IMMEDIATE`, confirmada quando `f` termina sem erro.
    async fn write<R, F>(&self, f: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&Transaction) -> rusqlite::Result<R> + Send + 'static,
    {
        self.writer
            .run(move |connection| {
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(sql_error)?;
                let result = f(&transaction).map_err(sql_error)?;
                transaction.commit().map_err(sql_error)?;
                Ok(result)
            })
            .await
    }

    /// Como `write`, mas a closure devolve `StoreError` (conflitos de domínio desfazem tudo).
    async fn mutate<R, F>(&self, f: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&Transaction) -> Result<R, StoreError> + Send + 'static,
    {
        self.writer
            .run(move |connection| {
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(sql_error)?;
                let result = f(&transaction)?;
                transaction.commit().map_err(sql_error)?;
                Ok(result)
            })
            .await
    }

    async fn read<R, F>(&self, f: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
    {
        self.readers
            .run(move |connection| f(connection).map_err(sql_error))
            .await
    }
}

impl Store for SqliteStore {
    async fn ready(&self) -> Result<ReadyReport, StoreError> {
        let applied = self.read(migrations::applied).await?;
        Ok(ReadyReport {
            migrations: MigrationStatus {
                applied,
                expected: migrations::expected(),
            },
        })
    }

    async fn diagnose(&self) -> Result<DoctorReport, StoreError> {
        let mut report = self
            .read(|connection| {
                let mut statement = connection.prepare("PRAGMA integrity_check")?;
                let problems = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .take(10)
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let integrity = problems.join("; ");
                Ok(DoctorReport {
                    integrity,
                    migrations: MigrationStatus {
                        applied: migrations::applied(connection)?,
                        expected: migrations::expected(),
                    },
                    sqlite_version: connection
                        .query_row("SELECT sqlite_version()", [], |row| row.get(0))?,
                    vec_version: connection
                        .query_row("SELECT vec_version()", [], |row| row.get(0))
                        .ok(),
                    fts5: connection.query_row(
                        "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
                        [],
                        |row| row.get(0),
                    )?,
                    journal_mode: connection
                        .query_row("PRAGMA journal_mode", [], |row| row.get(0))?,
                    memory_fts: None,
                })
            })
            .await?;
        report.memory_fts = self
            .writer
            .run(|connection| {
                let exists: bool = connection
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'memory_fts')",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(sql_error)?;
                if !exists {
                    return Ok(None);
                }
                Ok(Some(
                    match connection.execute(
                        "INSERT INTO memory_fts (memory_fts, rank) VALUES ('integrity-check', 1)",
                        [],
                    ) {
                        Ok(_) => "ok".to_owned(),
                        Err(error) => error.to_string(),
                    },
                ))
            })
            .await?;
        Ok(report)
    }

    async fn meta(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let key = key.to_owned();
        self.read(move |connection| {
            connection
                .query_row(
                    "SELECT value FROM schema_meta WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()
        })
        .await
    }

    async fn set_meta(&self, key: &str, value: &[u8]) -> Result<(), StoreError> {
        let key = key.to_owned();
        let value = value.to_vec();
        self.write(move |transaction| {
            transaction.execute(
                "INSERT INTO schema_meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
        .await
    }

    async fn find_auth_token(&self, id: &str) -> Result<Option<AuthTokenRow>, StoreError> {
        let id = id.to_owned();
        self.read(move |connection| identities::find_auth_token(connection, &id))
            .await
    }

    async fn count_tokens(&self) -> Result<u64, StoreError> {
        self.read(identities::count_tokens).await
    }

    async fn identity_counts(&self) -> Result<IdentityCounts, StoreError> {
        self.read(identities::identity_counts).await
    }

    async fn list_apps(&self, page: PageRequest) -> Result<Page<App>, StoreError> {
        self.read(move |connection| identities::list_apps(connection, &page))
            .await
    }

    async fn get_app(&self, lookup: Lookup) -> Result<Option<App>, StoreError> {
        self.read(move |connection| identities::get_app(connection, &lookup))
            .await
    }

    async fn create_app(&self, new: NewApp, context: WriteContext) -> Result<App, StoreError> {
        self.mutate(move |transaction| identities::create_app(transaction, &new, &context))
            .await
    }

    async fn update_app(
        &self,
        id: String,
        patch: AppPatch,
        context: WriteContext,
    ) -> Result<App, StoreError> {
        self.mutate(move |transaction| identities::update_app(transaction, &id, &patch, &context))
            .await
    }

    async fn delete_app(&self, id: String, context: WriteContext) -> Result<App, StoreError> {
        self.mutate(move |transaction| identities::delete_app(transaction, &id, &context))
            .await
    }

    async fn list_agents(
        &self,
        app_id: Option<String>,
        page: PageRequest,
    ) -> Result<Page<Agent>, StoreError> {
        self.read(move |connection| identities::list_agents(connection, app_id.as_deref(), &page))
            .await
    }

    async fn get_agent(&self, lookup: AgentLookup) -> Result<Option<Agent>, StoreError> {
        self.read(move |connection| identities::get_agent(connection, &lookup))
            .await
    }

    async fn create_agent(
        &self,
        new: NewAgent,
        context: WriteContext,
    ) -> Result<Agent, StoreError> {
        self.mutate(move |transaction| identities::create_agent(transaction, &new, &context))
            .await
    }

    async fn update_agent(
        &self,
        id: String,
        patch: AgentPatch,
        context: WriteContext,
    ) -> Result<Agent, StoreError> {
        self.mutate(move |transaction| identities::update_agent(transaction, &id, &patch, &context))
            .await
    }

    async fn delete_agent(&self, id: String, context: WriteContext) -> Result<Agent, StoreError> {
        self.mutate(move |transaction| identities::delete_agent(transaction, &id, &context))
            .await
    }

    async fn list_tokens(
        &self,
        filter: TokenFilter,
        page: PageRequest,
    ) -> Result<Page<Token>, StoreError> {
        self.read(move |connection| identities::list_tokens(connection, &filter, &page))
            .await
    }

    async fn get_token(&self, id: String) -> Result<Option<Token>, StoreError> {
        self.read(move |connection| identities::get_token(connection, &id))
            .await
    }

    async fn create_token(
        &self,
        new: NewToken,
        context: WriteContext,
    ) -> Result<Token, StoreError> {
        self.mutate(move |transaction| identities::create_token(transaction, &new, &context))
            .await
    }

    async fn update_token(
        &self,
        id: String,
        patch: TokenPatch,
        context: WriteContext,
    ) -> Result<Token, StoreError> {
        self.mutate(move |transaction| identities::update_token(transaction, &id, &patch, &context))
            .await
    }

    async fn revoke_token(&self, id: String, context: WriteContext) -> Result<Token, StoreError> {
        self.mutate(move |transaction| identities::revoke_token(transaction, &id, &context))
            .await
    }

    async fn bootstrap(
        &self,
        app: NewApp,
        token: NewToken,
        context: WriteContext,
    ) -> Result<Option<Token>, StoreError> {
        self.mutate(move |transaction| identities::bootstrap(transaction, &app, &token, &context))
            .await
    }

    async fn touch_tokens(&self, batch: Vec<(String, String)>) -> Result<(), StoreError> {
        if batch.is_empty() {
            return Ok(());
        }
        self.write(move |transaction| identities::touch_tokens(transaction, &batch))
            .await
    }

    async fn network(&self) -> Result<Network, StoreError> {
        self.read(channels::network).await
    }

    async fn get_channel(&self, lookup: Lookup) -> Result<Option<Channel>, StoreError> {
        self.read(move |connection| channels::get_channel(connection, &lookup))
            .await
    }

    async fn create_channel(
        &self,
        new: NewChannel,
        context: WriteContext,
    ) -> Result<Channel, StoreError> {
        self.mutate(move |transaction| channels::create_channel(transaction, &new, &context))
            .await
    }

    async fn update_channel(
        &self,
        id: String,
        patch: ChannelPatch,
        context: WriteContext,
    ) -> Result<Channel, StoreError> {
        self.mutate(move |transaction| channels::update_channel(transaction, &id, &patch, &context))
            .await
    }

    async fn set_channel_archived(
        &self,
        id: String,
        archived: bool,
        context: WriteContext,
    ) -> Result<Channel, StoreError> {
        self.mutate(move |transaction| {
            channels::set_channel_archived(transaction, &id, archived, &context)
        })
        .await
    }

    async fn delete_channel(
        &self,
        id: String,
        context: WriteContext,
    ) -> Result<Channel, StoreError> {
        self.mutate(move |transaction| channels::delete_channel(transaction, &id, &context))
            .await
    }

    async fn create_subscription(
        &self,
        new: NewSubscription,
        context: WriteContext,
    ) -> Result<Subscription, StoreError> {
        self.mutate(move |transaction| channels::create_subscription(transaction, &new, &context))
            .await
    }

    async fn update_subscription(
        &self,
        id: String,
        patch: SubscriptionPatch,
        context: WriteContext,
    ) -> Result<Subscription, StoreError> {
        self.mutate(move |transaction| {
            channels::update_subscription(transaction, &id, &patch, &context)
        })
        .await
    }

    async fn delete_subscription(
        &self,
        id: String,
        context: WriteContext,
    ) -> Result<Subscription, StoreError> {
        self.mutate(move |transaction| channels::delete_subscription(transaction, &id, &context))
            .await
    }

    async fn update_channel_settings(
        &self,
        channel_id: String,
        changes: Vec<SettingChange>,
        context: WriteContext,
    ) -> Result<ChannelSettingsMap, StoreError> {
        self.mutate(move |transaction| {
            channels::update_channel_settings(transaction, &channel_id, &changes, &context)
        })
        .await
    }

    async fn get_memory(&self, id: String) -> Result<Option<Memory>, StoreError> {
        self.read(move |connection| memories::get_memory(connection, &id))
            .await
    }

    async fn find_memory_by_hash(
        &self,
        channel_id: String,
        content_hash: Vec<u8>,
        exclude: Option<String>,
    ) -> Result<Option<String>, StoreError> {
        self.read(move |connection| {
            memories::find_by_hash(connection, &channel_id, &content_hash, exclude.as_deref())
        })
        .await
    }

    async fn list_memories(
        &self,
        filter: MemoryFilter,
        page: PageRequest,
    ) -> Result<Page<Memory>, StoreError> {
        self.read(move |connection| memories::list_memories(connection, &filter, &page))
            .await
    }

    async fn memory_history(
        &self,
        id: String,
        page: PageRequest,
    ) -> Result<Page<MemoryRevision>, StoreError> {
        self.read(move |connection| memories::memory_history(connection, &id, &page))
            .await
    }

    async fn get_revision(
        &self,
        id: String,
        version: i64,
    ) -> Result<Option<MemoryRevision>, StoreError> {
        self.read(move |connection| memories::get_revision(connection, &id, version))
            .await
    }

    async fn search_text(&self, query: TextQuery) -> Result<Vec<Memory>, StoreError> {
        self.read(move |connection| memories::search_text(connection, &query))
            .await
    }

    async fn create_memory(
        &self,
        new: NewMemory,
        context: WriteContext,
    ) -> Result<MemoryWrite, StoreError> {
        self.mutate(move |transaction| memories::create_memory(transaction, &new, &context))
            .await
    }

    async fn update_memory(
        &self,
        target: MemoryTarget,
        patch: MemoryPatch,
        context: WriteContext,
    ) -> Result<MemoryWrite, StoreError> {
        self.mutate(move |transaction| {
            memories::update_memory(transaction, &target, &patch, &context)
        })
        .await
    }

    async fn set_memory_status(
        &self,
        target: MemoryTarget,
        action: StatusAction,
        context: WriteContext,
    ) -> Result<Memory, StoreError> {
        self.mutate(move |transaction| {
            memories::set_memory_status(transaction, &target, action, &context)
        })
        .await
    }

    async fn move_memory(
        &self,
        target: MemoryTarget,
        to_channel_id: String,
        dedupe: bool,
        context: WriteContext,
    ) -> Result<Memory, StoreError> {
        self.mutate(move |transaction| {
            memories::move_memory(transaction, &target, &to_channel_id, dedupe, &context)
        })
        .await
    }

    async fn delete_memory(
        &self,
        target: MemoryTarget,
        context: WriteContext,
    ) -> Result<Memory, StoreError> {
        self.mutate(move |transaction| memories::delete_memory(transaction, &target, &context))
            .await
    }

    async fn bump_retrievals(&self, batch: Vec<RetrievalBump>) -> Result<(), StoreError> {
        if batch.is_empty() {
            return Ok(());
        }
        self.write(move |transaction| memories::bump_retrievals(transaction, &batch))
            .await
    }

    async fn channel_stats(&self, channel_id: String) -> Result<Option<ChannelStats>, StoreError> {
        self.read(move |connection| memories::channel_stats(connection, &channel_id))
            .await
    }

    async fn context_memories(
        &self,
        channel_ids: Vec<String>,
    ) -> Result<ContextSnapshot, StoreError> {
        self.read(move |connection| context::context_memories(connection, &channel_ids))
            .await
    }

    async fn bump_injections(&self, batch: Vec<InjectionBump>) -> Result<(), StoreError> {
        if batch.is_empty() {
            return Ok(());
        }
        self.write(move |transaction| context::bump_injections(transaction, &batch))
            .await
    }

    async fn append_usage(&self, batch: Vec<UsageEntry>) -> Result<(), StoreError> {
        if batch.is_empty() {
            return Ok(());
        }
        self.write(move |transaction| context::append_usage(transaction, &batch))
            .await
    }

    async fn category_counts(&self) -> Result<Vec<(String, u64)>, StoreError> {
        self.read(memories::category_counts).await
    }

    async fn get_settings(&self) -> Result<Vec<SettingRow>, StoreError> {
        self.read(settings::get_settings).await
    }

    async fn update_settings(
        &self,
        changes: Vec<SettingChange>,
        retained_categories: Option<Vec<String>>,
        context: WriteContext,
    ) -> Result<Vec<SettingRow>, StoreError> {
        self.mutate(move |transaction| {
            settings::update_settings(
                transaction,
                &changes,
                retained_categories.as_deref(),
                &context,
            )
        })
        .await
    }

    async fn find_idempotency(
        &self,
        token_id: &str,
        key: &str,
    ) -> Result<Option<IdemRow>, StoreError> {
        let token_id = token_id.to_owned();
        let key = key.to_owned();
        self.read(move |connection| journal::find(connection, &token_id, &key))
            .await
    }

    async fn purge_idempotency(&self, before: String) -> Result<u64, StoreError> {
        self.write(move |transaction| {
            transaction
                .execute(
                    "DELETE FROM idempotency WHERE created_at < ?1",
                    params![before],
                )
                .map(|count| count as u64)
        })
        .await
    }

    async fn list_audit(
        &self,
        filter: AuditFilter,
        page: PageRequest,
    ) -> Result<Page<AuditEvent>, StoreError> {
        self.read(move |connection| settings::list_audit(connection, &filter, &page))
            .await
    }
}

fn configure_writer(connection: &Connection) -> rusqlite::Result<()> {
    let mode: String =
        connection.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "journal_mode={mode}"
        )));
    }
    configure_common(connection)
}

fn configure_reader(connection: &Connection) -> rusqlite::Result<()> {
    configure_common(connection)
}

fn configure_common(connection: &Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Violação de unicidade (ou de chave primária) vira `Conflict(code)`; o resto, `sql_error`.
fn unique_conflict(error: rusqlite::Error, code: &'static str) -> StoreError {
    if let rusqlite::Error::SqliteFailure(failure, _) = &error
        && failure.code == ErrorCode::ConstraintViolation
        && matches!(
            failure.extended_code,
            rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE | rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
        )
    {
        return StoreError::Conflict(code);
    }
    sql_error(error)
}

/// Erros transitórios ou de I/O viram `Unavailable`; o resto vira `Internal` sem dados.
fn sql_error(error: rusqlite::Error) -> StoreError {
    match &error {
        rusqlite::Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                ErrorCode::DatabaseBusy
                    | ErrorCode::DatabaseLocked
                    | ErrorCode::CannotOpen
                    | ErrorCode::SystemIoFailure
                    | ErrorCode::DiskFull
                    | ErrorCode::OutOfMemory
            ) =>
        {
            StoreError::Unavailable
        }
        _ => StoreError::Internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    async fn store() -> (tempfile::TempDir, SqliteStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("engramind.db"), 2).expect("open");
        store.migrate().await.expect("migrate");
        (dir, store)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Pragmas {
        journal_mode: String,
        synchronous: i64,
        busy_timeout: i64,
        foreign_keys: i64,
    }

    fn pragmas(connection: &Connection) -> rusqlite::Result<Pragmas> {
        let get = |name: &str| {
            connection.query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, i64>(0))
        };
        Ok(Pragmas {
            journal_mode: connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?,
            synchronous: get("synchronous")?,
            busy_timeout: get("busy_timeout")?,
            foreign_keys: get("foreign_keys")?,
        })
    }

    #[tokio::test]
    async fn every_connection_has_the_required_pragmas() {
        let (_dir, store) = store().await;
        let expected = Pragmas {
            journal_mode: "wal".to_owned(),
            synchronous: 1,
            busy_timeout: 5000,
            foreign_keys: 1,
        };
        let writer = store
            .writer
            .run(|connection| pragmas(connection).map_err(sql_error))
            .await
            .expect("writer pragmas");
        assert_eq!(writer, expected);
        for _ in 0..4 {
            assert_eq!(store.read(pragmas).await.expect("reader pragmas"), expected);
        }
    }

    #[tokio::test]
    async fn sqlite_vec_and_fts5_are_available() {
        let (_dir, store) = store().await;
        let version: String = store
            .read(|connection| connection.query_row("SELECT vec_version()", [], |row| row.get(0)))
            .await
            .expect("vec_version");
        assert!(version.starts_with('v'), "{version}");

        let hits: i64 = store
            .write(|transaction| {
                transaction.execute_batch(
                    "CREATE VIRTUAL TABLE docs USING fts5(text, tokenize = 'unicode61 remove_diacritics 2');
                     INSERT INTO docs (text) VALUES ('Configuração do serviço');
                     CREATE VIRTUAL TABLE vectors USING vec0(embedding float[2]);
                     INSERT INTO vectors (rowid, embedding) VALUES (1, '[0.1, 0.2]');",
                )?;
                transaction.query_row(
                    "SELECT count(*) FROM docs WHERE docs MATCH 'configuracao servico'",
                    [],
                    |row| row.get(0),
                )
            })
            .await
            .expect("fts5 + vec0");
        assert_eq!(hits, 1);
    }

    #[tokio::test]
    async fn readers_are_read_only() {
        let (_dir, store) = store().await;
        let result = store
            .read(|connection| {
                connection.execute("INSERT INTO schema_meta VALUES ('k', x'00')", [])
            })
            .await;
        assert!(matches!(result, Err(StoreError::Internal(_))), "{result:?}");
    }

    #[tokio::test]
    async fn concurrent_writes_are_serialized() {
        let (_dir, store) = store().await;
        store
            .write(|transaction| {
                transaction.execute_batch(
                    "CREATE TABLE counter (n INTEGER NOT NULL); INSERT INTO counter VALUES (0);",
                )
            })
            .await
            .expect("table");
        let store = Arc::new(store);
        let mut tasks = Vec::new();
        for _ in 0..50 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .write(|transaction| {
                        let n: i64 =
                            transaction.query_row("SELECT n FROM counter", [], |row| row.get(0))?;
                        transaction.execute("UPDATE counter SET n = ?1", [n + 1])?;
                        Ok(())
                    })
                    .await
            }));
        }
        for task in tasks {
            task.await.expect("join").expect("write");
        }
        let n: i64 = store
            .read(|connection| connection.query_row("SELECT n FROM counter", [], |row| row.get(0)))
            .await
            .expect("read");
        assert_eq!(n, 50);
    }

    #[tokio::test]
    async fn a_failed_write_rolls_back_and_a_panic_keeps_the_writer_alive() {
        let (_dir, store) = store().await;
        let failed = store
            .write(|transaction| {
                transaction.execute("INSERT INTO schema_meta VALUES ('a', x'01')", [])?;
                transaction.execute("INSERT INTO tabela_inexistente VALUES (1)", [])?;
                Ok(())
            })
            .await;
        assert!(failed.is_err());
        assert_eq!(store.meta("a").await.expect("meta"), None);

        let panicked = store.write::<(), _>(|_| panic!("falha proposital")).await;
        assert!(matches!(panicked, Err(StoreError::Internal(_))));
        store.set_meta("b", b"ok").await.expect("writer alive");
        assert_eq!(store.meta("b").await.expect("meta"), Some(b"ok".to_vec()));
    }
}
