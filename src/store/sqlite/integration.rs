//! Consultas da integração (008): uso agregado, resumos de status e retenção.

use rusqlite::{Connection, Transaction, params};

use crate::core::model::{
    LlmSummary, RetentionTable, UsageAggregate, UsageGroup, UsageQuery, WebhookCounts,
};

fn count(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

pub(super) fn usage_report(
    connection: &Connection,
    query: &UsageQuery,
) -> rusqlite::Result<Vec<UsageAggregate>> {
    let sql = match query.group {
        UsageGroup::Day => {
            "SELECT substr(at, 1, 10), count(*), coalesce(sum(json_array_length(memory_ids)), 0),
                    coalesce(sum(rendered_tokens), 0)
             FROM usage_log WHERE at >= ?1 AND at < ?2
             GROUP BY 1 ORDER BY 1"
        }
        UsageGroup::Agent => {
            "SELECT coalesce(agent_id, app_id), count(*),
                    coalesce(sum(json_array_length(memory_ids)), 0),
                    coalesce(sum(rendered_tokens), 0)
             FROM usage_log WHERE at >= ?1 AND at < ?2
             GROUP BY 1 ORDER BY 1"
        }
        UsageGroup::Operation => {
            "SELECT op, count(*), coalesce(sum(json_array_length(memory_ids)), 0),
                    coalesce(sum(rendered_tokens), 0)
             FROM usage_log WHERE at >= ?1 AND at < ?2
             GROUP BY 1 ORDER BY 1"
        }
        // Uma requisição conta em cada canal que alcançou; as memórias, no canal atual delas.
        UsageGroup::Channel => {
            "SELECT c.value, count(*),
                    coalesce(sum((SELECT count(*) FROM json_each(u.memory_ids) m
                                  JOIN memories mm ON mm.id = m.value
                                  WHERE mm.channel_id = c.value)), 0),
                    coalesce(sum(u.rendered_tokens), 0)
             FROM usage_log u, json_each(u.channel_ids) c
             WHERE u.at >= ?1 AND u.at < ?2
             GROUP BY 1 ORDER BY 1"
        }
    };
    let mut statement = connection.prepare(sql)?;
    statement
        .query_map(params![query.since, query.until], |row| {
            Ok(UsageAggregate {
                key: row.get(0)?,
                requests: count(row.get(1)?),
                memories: count(row.get(2)?),
                rendered_tokens: count(row.get(3)?),
            })
        })?
        .collect()
}

pub(super) fn llm_summary(
    connection: &Connection,
    purpose: &str,
    since: &str,
) -> rusqlite::Result<LlmSummary> {
    connection.query_row(
        "SELECT count(*), coalesce(sum(outcome <> 'success'), 0), coalesce(sum(cost_usd), 0.0)
         FROM llm_calls WHERE purpose = ?1 AND at >= ?2",
        params![purpose, since],
        |row| {
            Ok(LlmSummary {
                calls: count(row.get(0)?),
                failures: count(row.get(1)?),
                cost_usd: row.get(2)?,
            })
        },
    )
}

pub(super) fn webhook_counts(
    connection: &Connection,
    since: &str,
) -> rusqlite::Result<WebhookCounts> {
    connection.query_row(
        "SELECT coalesce(sum(state IN ('queued', 'running')), 0),
                coalesce(sum(state = 'done' AND outcome = 'delivered' AND finished_at >= ?1), 0),
                coalesce(sum(state = 'failed' AND finished_at >= ?1), 0)
         FROM jobs WHERE kind = 'webhook'",
        params![since],
        |row| {
            Ok(WebhookCounts {
                pending: count(row.get(0)?),
                delivered: count(row.get(1)?),
                failed: count(row.get(2)?),
            })
        },
    )
}

pub(super) fn database_size(connection: &Connection) -> rusqlite::Result<u64> {
    let pages: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let size: i64 = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    Ok(count(pages.saturating_mul(size)))
}

pub(super) fn purge_before(
    transaction: &Transaction,
    table: RetentionTable,
    before: &str,
    limit: u32,
) -> rusqlite::Result<u64> {
    let sql = match table {
        RetentionTable::UsageLog => {
            "DELETE FROM usage_log WHERE id IN
                 (SELECT id FROM usage_log WHERE at < ?1 ORDER BY at LIMIT ?2)"
        }
        RetentionTable::LlmCalls => {
            "DELETE FROM llm_calls WHERE id IN
                 (SELECT id FROM llm_calls WHERE at < ?1 ORDER BY at LIMIT ?2)"
        }
        RetentionTable::AuditLog => {
            "DELETE FROM audit_log WHERE id IN
                 (SELECT id FROM audit_log WHERE at < ?1 ORDER BY at LIMIT ?2)"
        }
        RetentionTable::Jobs => {
            "DELETE FROM jobs WHERE id IN
                 (SELECT id FROM jobs WHERE state IN ('done', 'failed') AND finished_at < ?1
                  LIMIT ?2)"
        }
        RetentionTable::Idempotency => {
            "DELETE FROM idempotency WHERE rowid IN
                 (SELECT rowid FROM idempotency WHERE created_at < ?1 LIMIT ?2)"
        }
    };
    transaction
        .execute(sql, params![before, limit])
        .map(|removed| removed as u64)
}
