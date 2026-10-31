//! Fila `jobs`, cofre `secrets` e registro `llm_calls` no SQLite (006). O cofre só guarda e
//! devolve bytes cifrados; a cifra fica no `core`.

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, types::Value as SqlValue};
use serde_json::{Value, json};

use super::{
    journal::{self, Entry},
    sql_error,
};
use crate::{
    core::{
        model::{
            Job, JobCount, JobFilter, JobFinish, JobState, LlmActivity, LlmCall, LlmCallFilter,
            NewLlmCall, SecretView, StoredSecret, WriteContext,
        },
        pagination::{Cursor, Page, PageRequest},
    },
    store::StoreError,
};

const JOB_COLUMNS: &str = "id, kind, payload_json, state, attempts, run_after, locked_until, \
     last_error, outcome, created_at, finished_at";

fn job_from_row(row: &Row<'_>) -> rusqlite::Result<Job> {
    let payload: String = row.get(2)?;
    let state: String = row.get(3)?;
    Ok(Job {
        id: row.get(0)?,
        kind: row.get(1)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        state: JobState::parse(&state).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                format!("estado de job invalido: {state}").into(),
            )
        })?,
        attempts: row.get(4)?,
        run_after: row.get(5)?,
        locked_until: row.get(6)?,
        last_error: row.get(7)?,
        outcome: row.get(8)?,
        created_at: row.get(9)?,
        finished_at: row.get(10)?,
    })
}

/// Pega o job pronto mais antigo do tipo, ou um `running` com lease vencido, e conta a
/// tentativa (FR-002).
pub(super) fn claim(
    transaction: &Transaction,
    kind: &str,
    now: &str,
    lease_until: &str,
) -> rusqlite::Result<Option<Job>> {
    transaction
        .query_row(
            &format!(
                "UPDATE jobs SET state = 'running', attempts = attempts + 1, locked_until = ?3
                 WHERE id = (
                     SELECT id FROM jobs
                     WHERE kind = ?1
                       AND ((state = 'queued' AND run_after <= ?2)
                            OR (state = 'running' AND locked_until < ?2))
                     ORDER BY id LIMIT 1
                 )
                 RETURNING {JOB_COLUMNS}"
            ),
            params![kind, now, lease_until],
            job_from_row,
        )
        .optional()
}

pub(super) fn finish(
    transaction: &Transaction,
    id: i64,
    finish: &JobFinish,
    at: &str,
) -> rusqlite::Result<bool> {
    let changed = match finish {
        JobFinish::Done { outcome } => transaction.execute(
            "UPDATE jobs SET state = 'done', outcome = ?2, finished_at = ?3, locked_until = NULL
             WHERE id = ?1 AND state = 'running'",
            params![id, outcome, at],
        )?,
        JobFinish::Retry { error, run_after } => transaction.execute(
            "UPDATE jobs SET state = 'queued', last_error = ?2, run_after = ?3,
                 locked_until = NULL
             WHERE id = ?1 AND state = 'running'",
            params![id, error, run_after],
        )?,
        JobFinish::Fail { error } => transaction.execute(
            "UPDATE jobs SET state = 'failed', last_error = ?2, finished_at = ?3,
                 locked_until = NULL
             WHERE id = ?1 AND state = 'running'",
            params![id, error, at],
        )?,
    };
    Ok(changed > 0)
}

pub(super) fn newer_exists(
    connection: &Connection,
    kind: &str,
    memory_id: &str,
    after_id: i64,
) -> rusqlite::Result<bool> {
    connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM jobs
             WHERE kind = ?1 AND id > ?3 AND json_extract(payload_json, '$.memory_id') = ?2
         )",
        params![kind, memory_id, after_id],
        |row| row.get(0),
    )
}

pub(super) fn list(
    connection: &Connection,
    filter: &JobFilter,
    page: &PageRequest,
) -> rusqlite::Result<Page<Job>> {
    let mut conditions = Vec::new();
    let mut values = Vec::new();
    if let Some(state) = filter.state {
        conditions.push("state = ?".to_owned());
        values.push(SqlValue::Text(state.as_str().to_owned()));
    }
    if let Some(kind) = &filter.kind {
        conditions.push("kind = ?".to_owned());
        values.push(SqlValue::Text(kind.clone()));
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
        "SELECT {JOB_COLUMNS} FROM jobs{filter} ORDER BY id DESC LIMIT {}",
        page.limit + 1
    ))?;
    let items = statement
        .query_map(rusqlite::params_from_iter(values), job_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(by_id_page(items, page, |job| job.id))
}

fn by_id_page<T>(mut items: Vec<T>, page: &PageRequest, id: impl Fn(&T) -> i64) -> Page<T> {
    let next = if items.len() > page.limit as usize {
        items.truncate(page.limit as usize);
        items.last().map(|item| Cursor {
            key: String::new(),
            id: id(item).to_string(),
        })
    } else {
        None
    };
    Page { items, next }
}

fn load(transaction: &Transaction, id: i64) -> rusqlite::Result<Option<Job>> {
    transaction
        .query_row(
            &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?1"),
            params![id],
            job_from_row,
        )
        .optional()
}

fn job_audit(job: &Job) -> Value {
    json!({
        "id": job.id,
        "kind": job.kind,
        "state": job.state,
        "attempts": job.attempts,
    })
}

pub(super) fn retry(
    transaction: &Transaction,
    id: i64,
    context: &WriteContext,
) -> Result<Job, StoreError> {
    let before = load(transaction, id)
        .map_err(sql_error)?
        .ok_or(StoreError::NotFound)?;
    if before.state != JobState::Failed {
        return Err(StoreError::Conflict("JOB_NOT_RETRYABLE"));
    }
    transaction
        .execute(
            "UPDATE jobs SET state = 'queued', attempts = 0, run_after = ?2, locked_until = NULL,
                 finished_at = NULL, outcome = NULL
             WHERE id = ?1",
            params![id, context.at],
        )
        .map_err(sql_error)?;
    let after = load(transaction, id)
        .map_err(sql_error)?
        .ok_or(StoreError::NotFound)?;
    let entity_id = id.to_string();
    journal::audit(
        transaction,
        context,
        Entry {
            action: "job.retry",
            entity: "job",
            entity_id: Some(&entity_id),
            before: Some(job_audit(&before)),
            after: Some(job_audit(&after)),
        },
    )?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

pub(super) fn counts(connection: &Connection) -> rusqlite::Result<Vec<JobCount>> {
    let mut statement = connection.prepare(
        "SELECT kind, state, COUNT(*) FROM jobs GROUP BY kind, state ORDER BY kind, state",
    )?;
    statement
        .query_map([], |row| {
            let state: String = row.get(1)?;
            Ok((row.get::<_, String>(0)?, state, row.get::<_, i64>(2)?))
        })?
        .filter_map(|row| match row {
            Ok((kind, state, count)) => JobState::parse(&state).map(|state| {
                Ok(JobCount {
                    kind,
                    state,
                    count: u64::try_from(count).unwrap_or_default(),
                })
            }),
            Err(error) => Some(Err(error)),
        })
        .collect()
}

// ---------- segredos ----------

pub(super) fn get_secret(
    connection: &Connection,
    key: &str,
) -> rusqlite::Result<Option<StoredSecret>> {
    connection
        .query_row(
            "SELECT value_enc, updated_at FROM secrets WHERE key = ?1",
            params![key],
            |row| {
                Ok(StoredSecret {
                    value_enc: row.get(0)?,
                    updated_at: row.get(1)?,
                })
            },
        )
        .optional()
}

pub(super) fn list_secrets(connection: &Connection) -> rusqlite::Result<Vec<SecretView>> {
    let mut statement = connection.prepare("SELECT key, updated_at FROM secrets ORDER BY key")?;
    statement
        .query_map([], |row| {
            Ok(SecretView {
                key: row.get(0)?,
                configured: true,
                updated_at: Some(row.get(1)?),
            })
        })?
        .collect()
}

fn secret_view(transaction: &Transaction, key: &str) -> rusqlite::Result<SecretView> {
    let updated_at: Option<String> = transaction
        .query_row(
            "SELECT updated_at FROM secrets WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(SecretView {
        key: key.to_owned(),
        configured: updated_at.is_some(),
        updated_at,
    })
}

/// Grava ou apaga; a auditoria só registra `{ configured, updated_at }`.
pub(super) fn set_secret(
    transaction: &Transaction,
    key: &str,
    value_enc: Option<&[u8]>,
    context: &WriteContext,
) -> Result<SecretView, StoreError> {
    let before = secret_view(transaction, key).map_err(sql_error)?;
    match value_enc {
        Some(value) => transaction.execute(
            "INSERT INTO secrets (key, value_enc, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (key) DO UPDATE SET value_enc = excluded.value_enc,
                 updated_at = excluded.updated_at",
            params![key, value, context.at],
        ),
        None => transaction.execute("DELETE FROM secrets WHERE key = ?1", params![key]),
    }
    .map_err(sql_error)?;
    let after = secret_view(transaction, key).map_err(sql_error)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: if value_enc.is_some() {
                "secret.set"
            } else {
                "secret.delete"
            },
            entity: "secret",
            entity_id: Some(key),
            before: Some(journal::to_value(&before)?),
            after: Some(journal::to_value(&after)?),
        },
    )?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

// ---------- chamadas ao LLM ----------

pub(super) fn append_llm_calls(
    transaction: &Transaction,
    calls: &[NewLlmCall],
) -> rusqlite::Result<()> {
    let mut statement = transaction.prepare_cached(
        "INSERT INTO llm_calls (at, purpose, provider, model, input_tokens, output_tokens,
             cost_usd, duration_ms, outcome, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for call in calls {
        statement.execute(params![
            call.at,
            call.purpose,
            call.provider,
            call.model,
            call.input_tokens,
            call.output_tokens,
            call.cost_usd,
            call.duration_ms,
            call.outcome,
            call.error,
        ])?;
    }
    Ok(())
}

fn llm_call_from_row(row: &Row<'_>) -> rusqlite::Result<LlmCall> {
    Ok(LlmCall {
        id: row.get(0)?,
        at: row.get(1)?,
        purpose: row.get(2)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        input_tokens: row.get(5)?,
        output_tokens: row.get(6)?,
        cost_usd: row.get(7)?,
        duration_ms: row.get(8)?,
        outcome: row.get(9)?,
        error: row.get(10)?,
    })
}

pub(super) fn list_llm_calls(
    connection: &Connection,
    filter: &LlmCallFilter,
    page: &PageRequest,
) -> rusqlite::Result<Page<LlmCall>> {
    let mut conditions = Vec::new();
    let mut values = Vec::new();
    if let Some(since) = &filter.since {
        conditions.push("at >= ?".to_owned());
        values.push(SqlValue::Text(since.clone()));
    }
    if let Some(purpose) = &filter.purpose {
        conditions.push("purpose = ?".to_owned());
        values.push(SqlValue::Text(purpose.clone()));
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
        "SELECT id, at, purpose, provider, model, input_tokens, output_tokens, cost_usd,
                duration_ms, outcome, error
         FROM llm_calls{filter} ORDER BY id DESC LIMIT {}",
        page.limit + 1
    ))?;
    let items = statement
        .query_map(rusqlite::params_from_iter(values), llm_call_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(by_id_page(items, page, |call| call.id))
}

pub(super) fn llm_activity(
    connection: &Connection,
    purpose: &str,
) -> rusqlite::Result<LlmActivity> {
    connection.query_row(
        "SELECT MAX(CASE WHEN outcome = 'success' THEN at END),
                MAX(CASE WHEN outcome <> 'success' THEN at END)
         FROM llm_calls WHERE purpose = ?1",
        params![purpose],
        |row| {
            Ok(LlmActivity {
                last_success_at: row.get(0)?,
                last_failure_at: row.get(1)?,
            })
        },
    )
}
