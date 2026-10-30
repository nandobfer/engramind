use std::sync::LazyLock;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

/// Migrações embutidas no binário, em ordem. Nunca edite uma migração aplicada.
const SOURCES: &[&str] = &[include_str!("../../../migrations/0001_init.sql")];

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
}
