//! Implementação SQLite do `Store`: uma thread escritora e um pool de leitores (D-005),
//! WAL, FTS5 e `sqlite-vec`.

mod migrations;
mod readers;
mod vec_extension;
mod writer;

use std::{fs, path::Path, time::Duration};

use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::Serialize;

use self::{readers::Readers, writer::Writer};
use super::{DoctorReport, MigrationStatus, ReadyReport, Store, StoreError};

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
        self.read(|connection| {
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
                journal_mode: connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?,
            })
        })
        .await
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
