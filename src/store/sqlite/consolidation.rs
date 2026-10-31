//! Consolidação (009) no SQLite: leitura do canal para o planejador, fila `consolidate`, pares
//! recusados, notas pendentes e o resumo de cada execução. Fusão, desuso e `mark_verified`
//! ficam em `memories.rs`, junto das outras escritas de memória.

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Value, json};

use super::{
    journal::{self, Entry},
    sql_error,
};
use crate::{
    core::{
        consolidation::{
            ConsolidationActivity, ConsolidationEnqueue, ConsolidationInput, ConsolidationJobRef,
            ConsolidationMemory, ConsolidationRun, DeclinedPair, JOB_KIND, META_LAST_RUN,
            schedule::META_LAST_SCHEDULED,
        },
        model::{META_EMBEDDING_MODEL, WriteContext},
    },
    store::StoreError,
};

fn from_blob(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect()
}

/// Memórias `active` do canal com o vetor do modelo vigente e os pares recusados.
pub(super) fn input(
    connection: &Connection,
    channel_id: &str,
) -> rusqlite::Result<ConsolidationInput> {
    let model: Option<String> = connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = ?1",
            params![META_EMBEDDING_MODEL],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .filter(|model| !model.is_empty());
    let mut statement = connection.prepare(
        "SELECT m.id, m.category, m.title, m.text, m.content_hash, m.created_at, m.updated_at,
                m.importance, m.importance_pending, m.last_retrieved_at, m.last_injected_at,
                m.verified_at, m.stale_candidate, v.embedding
         FROM memories m
         LEFT JOIN memory_vectors v ON v.memory_id = m.id AND v.model_id = ?2
         WHERE m.channel_id = ?1 AND m.status = 'active'
         ORDER BY m.id",
    )?;
    let memories = statement
        .query_map(params![channel_id, model], |row| {
            let vector: Option<Vec<u8>> = row.get(13)?;
            Ok(ConsolidationMemory {
                id: row.get(0)?,
                category: row.get(1)?,
                title: row.get(2)?,
                text: row.get(3)?,
                content_hash: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                importance: row.get(7)?,
                importance_pending: row.get(8)?,
                last_retrieved_at: row.get(9)?,
                last_injected_at: row.get(10)?,
                verified_at: row.get(11)?,
                stale_candidate: row.get(12)?,
                vector: vector.map(|blob| from_blob(&blob)),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut statement = connection.prepare(
        "SELECT p.memory_a, p.memory_b, p.hash_a, p.hash_b
         FROM consolidation_pairs p JOIN memories m ON m.id = p.memory_a
         WHERE m.channel_id = ?1",
    )?;
    let declined = statement
        .query_map(params![channel_id], |row| {
            Ok(DeclinedPair {
                memory_a: row.get(0)?,
                memory_b: row.get(1)?,
                hash_a: row.get(2)?,
                hash_b: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ConsolidationInput { memories, declined })
}

/// Job `consolidate` `queued` ou `running` do canal.
fn pending_job(transaction: &Transaction, channel_id: &str) -> rusqlite::Result<Option<i64>> {
    transaction
        .query_row(
            "SELECT id FROM jobs
             WHERE kind = ?1 AND state IN ('queued', 'running')
               AND json_extract(payload_json, '$.channel_id') = ?2
             ORDER BY id LIMIT 1",
            params![JOB_KIND, channel_id],
            |row| row.get(0),
        )
        .optional()
}

/// Um job por canal sem `consolidate` pendente; o agendado grava o horário e o manual, a
/// auditoria e a idempotência, na mesma transação.
pub(super) fn enqueue(
    transaction: &Transaction,
    request: &ConsolidationEnqueue,
    context: &WriteContext,
) -> Result<ConsolidationRun, StoreError> {
    let tasks: Vec<&str> = request.tasks.iter().map(|task| task.as_str()).collect();
    let mut jobs = Vec::new();
    for (id, slug) in &request.channels {
        if let Some(job_id) = pending_job(transaction, id).map_err(sql_error)? {
            jobs.push(ConsolidationJobRef {
                channel: slug.clone(),
                job_id,
                already_queued: true,
            });
            continue;
        }
        let payload = json!({ "channel_id": id, "tasks": tasks, "trigger": request.trigger });
        transaction
            .execute(
                "INSERT INTO jobs (kind, payload_json, run_after, created_at)
                 VALUES (?1, ?2, ?3, ?3)",
                params![JOB_KIND, payload.to_string(), context.at],
            )
            .map_err(sql_error)?;
        jobs.push(ConsolidationJobRef {
            channel: slug.clone(),
            job_id: transaction.last_insert_rowid(),
            already_queued: false,
        });
    }
    if let Some(at) = &request.scheduled_at {
        transaction
            .execute(
                "INSERT INTO schema_meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![META_LAST_SCHEDULED, at.as_bytes()],
            )
            .map_err(sql_error)?;
    }
    let run = ConsolidationRun {
        dry_run: false,
        channels: Vec::new(),
        jobs,
    };
    if request.trigger == "manual" {
        journal::audit(
            transaction,
            context,
            Entry {
                action: "consolidation.request",
                entity: "consolidation",
                entity_id: None,
                before: None,
                after: Some(json!({ "tasks": tasks, "jobs": run.jobs })),
            },
        )?;
    }
    journal::remember(transaction, context, &run)?;
    Ok(run)
}

pub(super) fn decline_pair(
    transaction: &Transaction,
    pair: &DeclinedPair,
    at: &str,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO consolidation_pairs (memory_a, memory_b, hash_a, hash_b, decided_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (memory_a, memory_b) DO UPDATE SET
             hash_a = excluded.hash_a, hash_b = excluded.hash_b, decided_at = excluded.decided_at",
        params![pair.memory_a, pair.memory_b, pair.hash_a, pair.hash_b, at],
    )?;
    Ok(())
}

/// Notas em `importance_pending`, só para o conteúdo lido no plano; sem `version` (D-001).
pub(super) fn set_importance_pending(
    transaction: &Transaction,
    channel_id: &str,
    scores: &[(String, Vec<u8>, f64)],
) -> rusqlite::Result<u64> {
    let mut statement = transaction.prepare(
        "UPDATE memories SET importance_pending = ?1
         WHERE id = ?2 AND channel_id = ?3 AND content_hash = ?4 AND status = 'active'",
    )?;
    let mut written = 0;
    for (id, hash, score) in scores {
        written += statement.execute(params![score, id, channel_id, hash])? as u64;
    }
    Ok(written)
}

/// Fim da execução: remove os pares recusados cujo texto mudou, grava a auditoria
/// `consolidation.run` (contagens, sem textos) e o resumo por canal em `schema_meta`.
pub(super) fn finish(
    transaction: &Transaction,
    channel_id: &str,
    summary: &Value,
    context: &WriteContext,
) -> Result<(), StoreError> {
    transaction
        .execute(
            "DELETE FROM consolidation_pairs
             WHERE rowid IN (
                 SELECT p.rowid FROM consolidation_pairs p
                 JOIN memories a ON a.id = p.memory_a
                 JOIN memories b ON b.id = p.memory_b
                 WHERE a.channel_id = ?1
                   AND (a.content_hash <> p.hash_a OR b.content_hash <> p.hash_b
                        OR a.status <> 'active' OR b.status <> 'active'))",
            params![channel_id],
        )
        .map_err(sql_error)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "consolidation.run",
            entity: "channel",
            entity_id: Some(channel_id),
            before: None,
            after: Some(summary.clone()),
        },
    )?;
    let mut last: serde_json::Map<String, Value> = transaction
        .query_row(
            "SELECT value FROM schema_meta WHERE key = ?1",
            params![META_LAST_RUN],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(sql_error)?
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    last.insert(
        channel_id.to_owned(),
        json!({ "at": context.at, "summary": summary }),
    );
    transaction
        .execute(
            "INSERT INTO schema_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![META_LAST_RUN, Value::Object(last).to_string().as_bytes()],
        )
        .map_err(sql_error)?;
    Ok(())
}

/// Jobs pendentes e a soma das execuções desde `since` (auditoria `consolidation.run`).
pub(super) fn activity(
    connection: &Connection,
    since: &str,
) -> rusqlite::Result<ConsolidationActivity> {
    let (queued, running) = connection.query_row(
        "SELECT COALESCE(SUM(state = 'queued'), 0), COALESCE(SUM(state = 'running'), 0)
         FROM jobs WHERE kind = ?1 AND state IN ('queued', 'running')",
        params![JOB_KIND],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let mut totals = serde_json::Map::new();
    let mut statement = connection.prepare(
        "SELECT after_json FROM audit_log WHERE action = 'consolidation.run' AND at >= ?1",
    )?;
    let rows = statement.query_map(params![since], |row| row.get::<_, Option<String>>(0))?;
    for row in rows {
        let Some(json) = row? else { continue };
        let Ok(value) = serde_json::from_str::<Value>(&json) else {
            continue;
        };
        if let Some(report) = value["report"].as_object() {
            for (key, count) in report {
                let entry = totals.entry(key.clone()).or_insert(json!(0));
                *entry = json!(entry.as_u64().unwrap_or(0) + count.as_u64().unwrap_or(0));
            }
        }
    }
    Ok(ConsolidationActivity {
        queued: queued.max(0) as u64,
        running: running.max(0) as u64,
        totals: Value::Object(totals),
    })
}
