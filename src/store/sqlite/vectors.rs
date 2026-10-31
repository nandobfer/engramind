//! Vetores das memórias (007): `memory_vectors` com varredura exata por `vec_distance_cosine`
//! do sqlite-vec, filtrada por modelo, dimensão, canal, categoria e status. O BLOB é o vetor
//! `f32` little-endian, sem cabeçalho (formato do sqlite-vec).

use rusqlite::{Connection, OptionalExtension, Transaction, params, types::Value as SqlValue};
use serde_json::json;

use super::{
    journal,
    memories::{MEMORY_COLUMNS, MEMORY_JOINS, memory_from_row, placeholders, texts},
    sql_error,
};
use crate::{
    core::model::{
        EmbedSource, EmbeddingSwitch, META_EMBEDDING_MODEL, META_EMBEDDING_REINDEX, ReindexJobs,
        ReindexPlan, ReindexState, VectorBatch, VectorCounts, VectorHit, VectorQuery, WriteContext,
    },
    store::StoreError,
};

pub(super) fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn meta_text(connection: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = ?1",
            params![key],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map(|value| value.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
}

fn set_meta_text(transaction: &Transaction, key: &str, value: &str) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO schema_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value.as_bytes()],
    )?;
    Ok(())
}

pub(super) fn reindex_state(connection: &Connection) -> rusqlite::Result<Option<ReindexState>> {
    Ok(meta_text(connection, META_EMBEDDING_REINDEX)?
        .and_then(|text| serde_json::from_str(&text).ok()))
}

pub(super) fn embed_sources(
    connection: &Connection,
    ids: &[String],
) -> rusqlite::Result<Vec<EmbedSource>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(&format!(
        "SELECT id, title, text, content_hash FROM memories WHERE id IN ({}) ORDER BY seq",
        placeholders(ids.len())
    ))?;
    statement
        .query_map(
            rusqlite::params_from_iter(texts(ids.iter().map(String::as_str))),
            |row| {
                Ok(EmbedSource {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    text: row.get(2)?,
                    content_hash: row.get(3)?,
                })
            },
        )?
        .collect()
}

/// Grava os vetores cujo conteúdo e título ainda são os lidos, se o modelo ainda for o
/// vigente; devolve quantos gravou.
pub(super) fn save_vectors(
    transaction: &Transaction,
    batch: &VectorBatch,
) -> rusqlite::Result<u64> {
    if meta_text(transaction, META_EMBEDDING_MODEL)?.as_deref() != Some(batch.model_id.as_str()) {
        return Ok(0);
    }
    let dims = i64::try_from(batch.dims).unwrap_or(i64::MAX);
    let mut saved = 0;
    for vector in &batch.vectors {
        let current = transaction
            .query_row(
                "SELECT 1 FROM memories WHERE id = ?1 AND content_hash = ?2 AND title = ?3",
                params![vector.memory_id, vector.content_hash, vector.title],
                |_| Ok(()),
            )
            .optional()?;
        if current.is_none() {
            continue;
        }
        transaction.execute(
            "INSERT INTO memory_vectors (memory_id, model_id, dims, embedding, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (memory_id) DO UPDATE SET model_id = excluded.model_id,
                 dims = excluded.dims, embedding = excluded.embedding,
                 updated_at = excluded.updated_at",
            params![
                vector.memory_id,
                batch.model_id,
                dims,
                to_blob(&vector.embedding),
                batch.at
            ],
        )?;
        transaction.execute(
            "UPDATE memories SET embedding_state = 'ready' WHERE id = ?1",
            params![vector.memory_id],
        )?;
        saved += 1;
    }
    Ok(saved)
}

/// `embedding_state` das memórias ainda `pending` (`failed` ou `off`).
pub(super) fn mark_embedding(
    transaction: &Transaction,
    ids: &[String],
    state: &str,
) -> rusqlite::Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let mut values = vec![SqlValue::Text(state.to_owned())];
    values.extend(texts(ids.iter().map(String::as_str)));
    let changed = transaction.execute(
        &format!(
            "UPDATE memories SET embedding_state = ? \
             WHERE embedding_state = 'pending' AND id IN ({})",
            placeholders(ids.len())
        ),
        rusqlite::params_from_iter(values),
    )?;
    Ok(changed as u64)
}

pub(super) fn search_vectors(
    connection: &Connection,
    query: &VectorQuery,
) -> rusqlite::Result<Vec<VectorHit>> {
    if query.embedding.is_empty() || query.channel_ids.is_empty() || query.statuses.is_empty() {
        return Ok(Vec::new());
    }
    let mut conditions = vec![
        "v.model_id = ?".to_owned(),
        "v.dims = ?".to_owned(),
        format!(
            "m.channel_id IN ({})",
            placeholders(query.channel_ids.len())
        ),
        format!("m.status IN ({})", placeholders(query.statuses.len())),
    ];
    let mut values = vec![
        SqlValue::Blob(to_blob(&query.embedding)),
        SqlValue::Text(query.model_id.clone()),
        SqlValue::Integer(i64::try_from(query.embedding.len()).unwrap_or(i64::MAX)),
    ];
    values.extend(texts(query.channel_ids.iter().map(String::as_str)));
    values.extend(texts(query.statuses.iter().map(|status| status.as_str())));
    if !query.categories.is_empty() {
        conditions.push(format!(
            "m.category IN ({})",
            placeholders(query.categories.len())
        ));
        values.extend(texts(query.categories.iter().map(String::as_str)));
    }
    if !query.exclude_ids.is_empty() {
        conditions.push(format!(
            "m.id NOT IN ({})",
            placeholders(query.exclude_ids.len())
        ));
        values.extend(texts(query.exclude_ids.iter().map(String::as_str)));
    }
    let mut statement = connection.prepare(&format!(
        "SELECT {MEMORY_COLUMNS}, vec_distance_cosine(v.embedding, ?) AS distance
         FROM memory_vectors v JOIN memories m ON m.id = v.memory_id
         {MEMORY_JOINS}
         WHERE {} ORDER BY distance, m.id LIMIT {}",
        conditions.join(" AND "),
        query.limit
    ))?;
    let distance_index = MEMORY_COLUMNS.split(", ").count();
    statement
        .query_map(rusqlite::params_from_iter(values), |row| {
            Ok(VectorHit {
                memory: memory_from_row(row)?,
                distance: row.get(distance_index)?,
            })
        })?
        .collect()
}

pub(super) fn vector_counts(
    connection: &Connection,
    model_id: &str,
    since: Option<&str>,
) -> rusqlite::Result<VectorCounts> {
    let memories: i64 =
        connection.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
    let (vectors, recent): (i64, i64) = connection.query_row(
        "SELECT COUNT(*), COALESCE(SUM(updated_at >= ?2), 0)
         FROM memory_vectors WHERE model_id = ?1",
        params![model_id, since.unwrap_or("")],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(VectorCounts {
        memories: memories.max(0) as u64,
        vectors: vectors.max(0) as u64,
        since: recent.max(0) as u64,
    })
}

/// Marca as memórias e enfileira os lotes `embed` da reindexação; devolve o estado gravado
/// (`None` quando não havia nada a vetorizar).
fn enqueue_reindex(
    transaction: &Transaction,
    model_id: &str,
    plan: &ReindexPlan,
    at: &str,
) -> rusqlite::Result<Option<ReindexState>> {
    let (sql, values) = if plan.all {
        ("SELECT id FROM memories ORDER BY seq", Vec::new())
    } else {
        (
            "SELECT m.id FROM memories m
             LEFT JOIN memory_vectors v ON v.memory_id = m.id AND v.model_id = ?1
             WHERE v.memory_id IS NULL ORDER BY m.seq",
            vec![SqlValue::Text(model_id.to_owned())],
        )
    };
    let ids: Vec<String> = transaction
        .prepare(sql)?
        .query_map(rusqlite::params_from_iter(values), |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    transaction.execute(
        "DELETE FROM schema_meta WHERE key = ?1",
        params![META_EMBEDDING_REINDEX],
    )?;
    if ids.is_empty() {
        return Ok(None);
    }
    for chunk in ids.chunks(plan.batch_size.max(1)) {
        let mut values = vec![SqlValue::Text("pending".to_owned())];
        values.extend(texts(chunk.iter().map(String::as_str)));
        transaction.execute(
            &format!(
                "UPDATE memories SET embedding_state = ? WHERE id IN ({})",
                placeholders(chunk.len())
            ),
            rusqlite::params_from_iter(values),
        )?;
        let payload = json!({ "memory_ids": chunk, "reindex": plan.started_at });
        transaction.execute(
            "INSERT INTO jobs (kind, payload_json, run_after, created_at) VALUES ('embed', ?1, ?2, ?2)",
            params![payload.to_string(), at],
        )?;
    }
    let state = ReindexState {
        model_id: model_id.to_owned(),
        started_at: plan.started_at.clone(),
        total: ids.len() as u64,
    };
    let text = serde_json::to_string(&state).unwrap_or_default();
    set_meta_text(transaction, META_EMBEDDING_REINDEX, &text)?;
    Ok(Some(state))
}

/// Troca o espaço dos vetores (configurações de embedding); devolve a reindexação criada.
pub(super) fn switch(
    transaction: &Transaction,
    switch: &EmbeddingSwitch,
    at: &str,
) -> rusqlite::Result<Option<ReindexState>> {
    match &switch.model_id {
        None => {
            set_meta_text(transaction, META_EMBEDDING_MODEL, "")?;
            transaction.execute(
                "DELETE FROM schema_meta WHERE key = ?1",
                params![META_EMBEDDING_REINDEX],
            )?;
            transaction.execute(
                "UPDATE memories SET embedding_state = 'off' WHERE embedding_state <> 'off'",
                [],
            )?;
            Ok(None)
        }
        Some(model_id) => {
            set_meta_text(transaction, META_EMBEDDING_MODEL, model_id)?;
            match &switch.reindex {
                Some(plan) => enqueue_reindex(transaction, model_id, plan, at),
                None => Ok(None),
            }
        }
    }
}

/// `reembed_all`: com uma reindexação de jobs pendentes → `Conflict("REEMBED_IN_PROGRESS")`.
pub(super) fn start_reindex(
    transaction: &Transaction,
    model_id: &str,
    plan: &ReindexPlan,
    context: &WriteContext,
) -> Result<ReindexState, StoreError> {
    if let Some(current) = reindex_state(transaction).map_err(sql_error)?
        && reindex_jobs(transaction, &current.started_at, None)
            .map_err(sql_error)?
            .pending
            > 0
    {
        return Err(StoreError::Conflict("REEMBED_IN_PROGRESS"));
    }
    set_meta_text(transaction, META_EMBEDDING_MODEL, model_id).map_err(sql_error)?;
    let state = enqueue_reindex(transaction, model_id, plan, &context.at)
        .map_err(sql_error)?
        .unwrap_or_else(|| ReindexState {
            model_id: model_id.to_owned(),
            started_at: plan.started_at.clone(),
            total: 0,
        });
    journal::audit(
        transaction,
        context,
        journal::Entry {
            action: "embeddings.reembed",
            entity: "embeddings",
            entity_id: Some(model_id),
            before: None,
            after: Some(json!({ "total": state.total, "started_at": state.started_at })),
        },
    )?;
    journal::remember(transaction, context, &state)?;
    Ok(state)
}

pub(super) fn reindex_jobs(
    connection: &Connection,
    started_at: &str,
    exclude_job: Option<i64>,
) -> rusqlite::Result<ReindexJobs> {
    connection.query_row(
        "SELECT COALESCE(SUM(state IN ('queued', 'running')), 0),
                COALESCE(SUM(state = 'failed'), 0)
         FROM jobs
         WHERE kind = 'embed' AND json_extract(payload_json, '$.reindex') = ?1 AND id <> ?2",
        params![started_at, exclude_job.unwrap_or(0)],
        |row| {
            Ok(ReindexJobs {
                pending: row.get::<_, i64>(0)?.max(0) as u64,
                failed: row.get::<_, i64>(1)?.max(0) as u64,
            })
        },
    )
}

/// Encerra a reindexação `started_at` (ou a limpeza sem reindexação, com `None`): apaga os
/// vetores de outros modelos e o registro. Devolve se encerrou.
pub(super) fn finish_reindex(
    transaction: &Transaction,
    model_id: &str,
    started_at: Option<&str>,
) -> rusqlite::Result<bool> {
    let current = reindex_state(transaction)?;
    match (started_at, &current) {
        (Some(tag), Some(state)) if state.started_at == tag => {}
        (None, None) => {}
        _ => return Ok(false),
    }
    if meta_text(transaction, META_EMBEDDING_MODEL)?.as_deref() != Some(model_id) {
        return Ok(false);
    }
    transaction.execute(
        "DELETE FROM memory_vectors WHERE model_id <> ?1",
        params![model_id],
    )?;
    transaction.execute(
        "DELETE FROM schema_meta WHERE key = ?1",
        params![META_EMBEDDING_REINDEX],
    )?;
    Ok(true)
}
