//! Auditoria e idempotência gravadas na mesma transação de cada mutação (FR-008, D-008).

use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;
use serde_json::Value;

use super::sql_error;
use crate::{
    core::model::{IdemRow, WriteContext},
    store::StoreError,
};

/// Uma linha de `audit_log` a gravar.
pub(super) struct Entry<'a> {
    pub action: &'a str,
    pub entity: &'a str,
    pub entity_id: Option<&'a str>,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

pub(super) fn to_value<T: Serialize>(value: &T) -> Result<Value, StoreError> {
    serde_json::to_value(value).map_err(|error| StoreError::Internal(error.to_string()))
}

pub(super) fn audit(
    transaction: &Transaction,
    context: &WriteContext,
    entry: Entry<'_>,
) -> Result<(), StoreError> {
    let json = |value: Option<Value>| value.map(|value| value.to_string());
    transaction
        .execute(
            "INSERT INTO audit_log
                 (at, app_id, agent_id, token_id, action, entity, entity_id, before_json, after_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                context.at,
                context.actor.app_id,
                context.actor.agent_id,
                context.actor.token_id,
                entry.action,
                entry.entity,
                entry.entity_id,
                json(entry.before),
                json(entry.after),
            ],
        )
        .map_err(sql_error)?;
    Ok(())
}

/// Reserva a chave e grava a resposta. Uma chave viva já gravada → `Conflict("IDEMPOTENCY_KEY_EXISTS")`,
/// e o `core` relê o registro.
pub(super) fn remember<T: Serialize>(
    transaction: &Transaction,
    context: &WriteContext,
    response: &T,
) -> Result<(), StoreError> {
    let Some(record) = &context.idempotency else {
        return Ok(());
    };
    transaction
        .execute(
            "DELETE FROM idempotency WHERE token_id = ?1 AND key = ?2 AND created_at < ?3",
            params![record.token_id, record.key, record.expired_before],
        )
        .map_err(sql_error)?;
    let response =
        serde_json::to_string(response).map_err(|error| StoreError::Internal(error.to_string()))?;
    transaction
        .execute(
            "INSERT INTO idempotency (token_id, key, request_hash, response_json, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.token_id,
                record.key,
                record.request_hash,
                response,
                record.status,
                context.at
            ],
        )
        .map_err(|error| super::unique_conflict(error, "IDEMPOTENCY_KEY_EXISTS"))?;
    Ok(())
}

pub(super) fn find(
    connection: &rusqlite::Connection,
    token_id: &str,
    key: &str,
) -> rusqlite::Result<Option<IdemRow>> {
    connection
        .query_row(
            "SELECT request_hash, response_json, status, created_at
             FROM idempotency WHERE token_id = ?1 AND key = ?2",
            params![token_id, key],
            |row| {
                Ok(IdemRow {
                    request_hash: row.get(0)?,
                    response_json: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .optional()
}
