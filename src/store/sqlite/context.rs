//! Leituras e lotes do contexto no SQLite: memórias e versões dos canais numa mesma transação
//! de leitura, `injected_count` e `usage_log` (005).

use rusqlite::{Connection, Transaction, params, params_from_iter};
use serde_json::json;

use crate::core::model::{ContextMemory, ContextSnapshot, InjectionBump, MemoryStatus, UsageEntry};

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

/// Memórias `active`/`permanent` dos canais e as versões deles, no mesmo instantâneo (WAL).
pub(super) fn context_memories(
    connection: &Connection,
    channel_ids: &[String],
) -> rusqlite::Result<ContextSnapshot> {
    if channel_ids.is_empty() {
        return Ok(ContextSnapshot::default());
    }
    let transaction = connection.unchecked_transaction()?;
    let marks = placeholders(channel_ids.len());
    let versions = transaction
        .prepare(&format!(
            "SELECT id, version FROM channels WHERE id IN ({marks}) ORDER BY id"
        ))?
        .query_map(params_from_iter(channel_ids), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let memories = transaction
        .prepare(&format!(
            "SELECT id, channel_id, category, title, text, status, importance, created_at
             FROM memories
             WHERE channel_id IN ({marks}) AND status IN ('active', 'permanent')"
        ))?
        .query_map(params_from_iter(channel_ids), |row| {
            let status: String = row.get(5)?;
            Ok(ContextMemory {
                id: row.get(0)?,
                channel_id: row.get(1)?,
                category: row.get(2)?,
                title: row.get(3)?,
                text: row.get(4)?,
                status: MemoryStatus::parse(&status).unwrap_or(MemoryStatus::Active),
                importance: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    transaction.finish()?;
    Ok(ContextSnapshot { versions, memories })
}

/// Soma `injected_count` e avança `last_injected_at` sem mudar `version` nem `updated_at`.
pub(super) fn bump_injections(
    transaction: &Transaction,
    batch: &[InjectionBump],
) -> rusqlite::Result<()> {
    let mut statement = transaction.prepare(
        "UPDATE memories SET injected_count = injected_count + ?2,
             last_injected_at = CASE
                 WHEN last_injected_at IS NULL OR last_injected_at < ?3 THEN ?3
                 ELSE last_injected_at END
         WHERE id = ?1",
    )?;
    for bump in batch {
        statement.execute(params![bump.memory_id, bump.count, bump.at])?;
    }
    Ok(())
}

pub(super) fn append_usage(
    transaction: &Transaction,
    batch: &[UsageEntry],
) -> rusqlite::Result<()> {
    let mut statement = transaction.prepare(
        "INSERT INTO usage_log
             (at, app_id, agent_id, token_id, op, channel_ids, memory_ids, rendered_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for entry in batch {
        statement.execute(params![
            entry.at,
            entry.app_id,
            entry.agent_id,
            entry.token_id,
            entry.op,
            json!(entry.channel_ids).to_string(),
            json!(entry.memory_ids).to_string(),
            entry.rendered_tokens,
        ])?;
    }
    Ok(())
}
