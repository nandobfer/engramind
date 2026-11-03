use std::sync::LazyLock;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

/// Migrações embutidas no binário, em ordem. Nunca edite uma migração aplicada.
const SOURCES: &[&str] = &[
    include_str!("../../../migrations/0001_init.sql"),
    include_str!("../../../migrations/0002_identities.sql"),
    include_str!("../../../migrations/0003_channels.sql"),
    include_str!("../../../migrations/0004_memories.sql"),
    include_str!("../../../migrations/0005_usage.sql"),
    include_str!("../../../migrations/0006_redator.sql"),
    include_str!("../../../migrations/0007_vectors.sql"),
    include_str!("../../../migrations/0008_integration.sql"),
    include_str!("../../../migrations/0009_consolidation.sql"),
    include_str!("../../../migrations/0010_app_tokens.sql"),
    include_str!("../../../migrations/0011_memory_fields.sql"),
    include_str!("../../../migrations/0012_channel_lifecycle.sql"),
];

static MIGRATIONS: LazyLock<Migrations<'static>> =
    LazyLock::new(|| Migrations::new(SOURCES.iter().map(|sql| M::up(sql)).collect()));

pub(super) fn expected() -> u32 {
    SOURCES.len() as u32
}

pub(super) fn applied(connection: &Connection) -> rusqlite::Result<u32> {
    connection.query_row("PRAGMA user_version", [], |row| row.get(0))
}

pub(super) fn to_latest(connection: &mut Connection) -> Result<(), rusqlite_migration::Error> {
    MIGRATIONS.to_latest(connection)
}

#[cfg(test)]
mod tests {
    #[test]
    fn migrations_are_valid() {
        super::MIGRATIONS.validate().expect("valid migrations");
    }

    /// A 0010 recria `tokens`: os tokens da 0009 (inclusive os de agente em produção) passam
    /// intactos e o novo `kind = 'app'` só vale sem agente.
    #[test]
    fn the_token_rebuild_keeps_every_token_and_accepts_app_tokens() {
        let mut connection = rusqlite::Connection::open_in_memory().expect("open");
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .expect("fk");
        super::MIGRATIONS
            .to_version(&mut connection, 9)
            .expect("0009");
        connection
            .execute_batch(
                "INSERT INTO apps (id, slug, name, created_at) VALUES ('app_1', 'opex', 'Opex', 't');
                 INSERT INTO agents (id, app_id, slug, name, created_at)
                     VALUES ('ag_1', 'app_1', 'arquiteto', 'Arquiteto', 't');
                 INSERT INTO tokens (id, app_id, agent_id, kind, scopes, secret_hash, created_at)
                     VALUES ('tk_d6d1b5d8', 'app_1', 'ag_1', 'agent', 'read,write,delete', x'01', 't');
                 INSERT INTO tokens (id, app_id, kind, scopes, label, secret_hash, created_at,
                                     revoked_at)
                     VALUES ('tk_00000001', 'app_1', 'admin', 'read', 'boot', x'02', 't', 'r');",
            )
            .expect("seed");
        super::MIGRATIONS
            .to_latest(&mut connection)
            .expect("latest");
        let rows: Vec<(String, Option<String>, String, Option<String>)> = connection
            .prepare("SELECT id, agent_id, kind, revoked_at FROM tokens ORDER BY id")
            .expect("prepare")
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("rows");
        assert_eq!(
            rows,
            vec![
                (
                    "tk_00000001".to_owned(),
                    None,
                    "admin".to_owned(),
                    Some("r".to_owned())
                ),
                (
                    "tk_d6d1b5d8".to_owned(),
                    Some("ag_1".to_owned()),
                    "agent".to_owned(),
                    None
                ),
            ]
        );
        let violations: i64 = connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .expect("fk check");
        assert_eq!(violations, 0);
        connection
            .execute(
                "INSERT INTO tokens (id, app_id, kind, scopes, secret_hash, created_at)
                 VALUES ('tk_2', 'app_1', 'app', 'read', x'03', 't')",
                [],
            )
            .expect("app token");
        for invalid in [
            "INSERT INTO tokens (id, app_id, agent_id, kind, scopes, secret_hash, created_at)
             VALUES ('tk_3', 'app_1', 'ag_1', 'app', 'read', x'04', 't')",
            "INSERT INTO tokens (id, app_id, kind, scopes, secret_hash, created_at)
             VALUES ('tk_4', 'app_1', 'agent', 'read', x'05', 't')",
            "INSERT INTO tokens (id, app_id, kind, scopes, secret_hash, created_at)
             VALUES ('tk_5', 'app_1', 'root', 'read', x'06', 't')",
        ] {
            assert!(connection.execute(invalid, []).is_err(), "{invalid}");
        }
        // O agente excluído leva os tokens dele (cascata preservada).
        connection
            .execute("DELETE FROM agents WHERE id = 'ag_1'", [])
            .expect("delete");
        let left: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tokens WHERE agent_id = 'ag_1'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(left, 0);
    }
}
