//! Configurações e auditoria no SQLite.

use rusqlite::{Connection, Row, Transaction, params, types::Value as SqlValue};

use super::{
    journal::{self, Entry},
    sql_error,
};
use crate::{
    core::{
        model::{AuditEvent, AuditFilter, SettingChange, SettingRow, WriteContext},
        pagination::{Cursor, Page, PageRequest},
    },
    store::StoreError,
};

pub(super) fn get_settings(connection: &Connection) -> rusqlite::Result<Vec<SettingRow>> {
    let mut statement = connection
        .prepare("SELECT key, value_json, updated_by_app, updated_at FROM settings ORDER BY key")?;
    statement
        .query_map([], |row| {
            Ok(SettingRow {
                key: row.get(0)?,
                value_json: row.get(1)?,
                updated_by_app: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?
        .collect()
}

fn parse(value: Option<String>) -> Result<Option<serde_json::Value>, StoreError> {
    value
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|error| StoreError::Internal(error.to_string()))
}

pub(super) fn update_settings(
    transaction: &Transaction,
    changes: &[SettingChange],
    context: &WriteContext,
) -> Result<Vec<SettingRow>, StoreError> {
    for change in changes {
        let before: Option<String> = rusqlite::OptionalExtension::optional(transaction.query_row(
            "SELECT value_json FROM settings WHERE key = ?1",
            params![change.key],
            |row| row.get(0),
        ))
        .map_err(sql_error)?;
        match &change.value_json {
            Some(value) => transaction.execute(
                "INSERT INTO settings (key, value_json, updated_by_app, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (key) DO UPDATE SET value_json = excluded.value_json,
                     updated_by_app = excluded.updated_by_app, updated_at = excluded.updated_at",
                params![change.key, value, context.actor.app_id, context.at],
            ),
            None => transaction.execute("DELETE FROM settings WHERE key = ?1", params![change.key]),
        }
        .map_err(sql_error)?;
        journal::audit(
            transaction,
            context,
            Entry {
                action: "settings.update",
                entity: "settings",
                entity_id: Some(&change.key),
                before: parse(before)?,
                after: parse(change.value_json.clone())?,
            },
        )?;
    }
    let rows = get_settings(transaction).map_err(sql_error)?;
    journal::remember(transaction, context, &rows)?;
    Ok(rows)
}

fn audit_from_row(row: &Row<'_>) -> rusqlite::Result<AuditEvent> {
    let json = |index: usize| -> rusqlite::Result<Option<serde_json::Value>> {
        let text: Option<String> = row.get(index)?;
        Ok(text.and_then(|text| serde_json::from_str(&text).ok()))
    };
    Ok(AuditEvent {
        id: row.get(0)?,
        at: row.get(1)?,
        app_id: row.get(2)?,
        agent_id: row.get(3)?,
        token_id: row.get(4)?,
        action: row.get(5)?,
        entity: row.get(6)?,
        entity_id: row.get(7)?,
        before: json(8)?,
        after: json(9)?,
    })
}

/// Do mais recente ao mais antigo; o cursor guarda o último `id`.
pub(super) fn list_audit(
    connection: &Connection,
    filter: &AuditFilter,
    page: &PageRequest,
) -> rusqlite::Result<Page<AuditEvent>> {
    let mut conditions = Vec::new();
    let mut values = Vec::new();
    for (column, value) in [
        ("entity", &filter.entity),
        ("entity_id", &filter.entity_id),
        ("action", &filter.action),
    ] {
        if let Some(value) = value {
            conditions.push(format!("{column} = ?"));
            values.push(SqlValue::Text(value.clone()));
        }
    }
    if let Some(after) = &page.after {
        conditions.push("id < ?".to_owned());
        values.push(SqlValue::Integer(after.id.parse().unwrap_or(0)));
    }
    let filter = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let mut statement = connection.prepare(&format!(
        "SELECT id, at, app_id, agent_id, token_id, action, entity, entity_id, before_json,
                after_json
         FROM audit_log{filter} ORDER BY id DESC LIMIT {}",
        page.limit + 1
    ))?;
    let mut items = statement
        .query_map(rusqlite::params_from_iter(values), audit_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let next = if items.len() > page.limit as usize {
        items.truncate(page.limit as usize);
        items.last().map(|event| Cursor {
            key: String::new(),
            id: event.id.to_string(),
        })
    } else {
        None
    };
    Ok(Page { items, next })
}
