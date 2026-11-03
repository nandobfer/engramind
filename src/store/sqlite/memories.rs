//! Memórias, revisões, busca textual e a fila de jobs no SQLite. Toda mutação grava, na mesma
//! transação, a memória, a revisão, a auditoria (só metadados e hashes, nunca o texto),
//! `memories.version`, `channels.version`, os jobs e a idempotência (desenho §6).

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, types::Value as SqlValue};
use serde_json::{Value, json};

use super::{
    identities::free_id,
    journal::{self, Entry},
    sql_error,
};
use crate::{
    core::{
        consolidation::job::{MemoryMerge, StaleOutcome, StaleWrite},
        ids,
        memory_text::hash_hex,
        model::{
            Author, ChannelStats, EmbeddingState, ImportCreated, ImportDuplicate, ImportResult,
            Memory, MemoryContent, MemoryCounts, MemoryFilter, MemoryPatch, MemoryRevision,
            MemoryStatus, MemoryTarget, MemoryWrite, NewImportedMemory, NewJob, NewMemory, Origin,
            PendingCounts, Redaction, RetrievalBump, StatusAction, TextQuery, TitleChange,
            TitleState, WriteContext,
        },
        pagination::{Cursor, Page, PageRequest},
    },
    store::StoreError,
};

pub(super) const MEMORY_COLUMNS: &str = "m.id, c.slug, m.channel_id, m.category, m.title, m.title_state, \
     m.text, m.status, m.importance, m.version, m.embedding_state, m.author_app_id, a.slug, \
     m.author_agent_id, ga.slug || '/' || g.slug, m.origin_source, m.origin_label, \
     m.retrieval_count, m.injected_count, m.last_retrieved_at, m.verified_at, m.created_at, \
     m.updated_at, m.archived_at, m.stale_candidate, m.merged_into, m.title_locked, m.key, \
     m.last_origin_source, m.last_origin_label, \
     (SELECT json_group_array(tag) FROM (SELECT tag FROM memory_tags WHERE memory_id = m.id \
     ORDER BY position))";
pub(super) const MEMORY_JOINS: &str = "JOIN channels c ON c.id = m.channel_id \
     LEFT JOIN apps a ON a.id = m.author_app_id \
     LEFT JOIN agents g ON g.id = m.author_agent_id \
     LEFT JOIN apps ga ON ga.id = g.app_id";

/// Autor das revisões gravadas sem app (não acontece com token; reservado ao sistema).
const SYSTEM_APP: &str = "system";

fn invalid(index: usize, message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, message.into())
}

fn status_at(row: &Row<'_>, index: usize) -> rusqlite::Result<MemoryStatus> {
    let value: String = row.get(index)?;
    MemoryStatus::parse(&value).ok_or_else(|| invalid(index, format!("status: {value}")))
}

pub(super) fn memory_from_row(row: &Row<'_>) -> rusqlite::Result<Memory> {
    let title_state: String = row.get(5)?;
    let embedding_state: String = row.get(10)?;
    let origin_source: Option<String> = row.get(15)?;
    let origin_label: Option<String> = row.get(16)?;
    Ok(Memory {
        id: row.get(0)?,
        channel: row.get(1)?,
        channel_id: row.get(2)?,
        category: row.get(3)?,
        title: row.get(4)?,
        title_state: TitleState::parse(&title_state)
            .ok_or_else(|| invalid(5, format!("title_state: {title_state}")))?,
        text: row.get(6)?,
        status: status_at(row, 7)?,
        importance: row.get(8)?,
        version: row.get(9)?,
        embedding_state: EmbeddingState::parse(&embedding_state)
            .ok_or_else(|| invalid(10, format!("embedding_state: {embedding_state}")))?,
        author: Author {
            app_id: row.get(11)?,
            app: row.get(12)?,
            agent_id: row.get(13)?,
            agent: row.get(14)?,
        },
        origin: origin_source.map(|source| Origin {
            source,
            label: origin_label,
        }),
        retrieval_count: row.get(17)?,
        injected_count: row.get(18)?,
        last_retrieved_at: row.get(19)?,
        verified_at: row.get(20)?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
        archived_at: row.get(23)?,
        stale_candidate: row.get(24)?,
        merged_into: row.get(25)?,
        title_locked: row.get(26)?,
        key: row.get(27)?,
        last_origin: origin_at(row, 28)?,
        tags: tags_at(row, 30)?,
    })
}

fn origin_at(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<Origin>> {
    let source: Option<String> = row.get(index)?;
    let label: Option<String> = row.get(index + 1)?;
    Ok(source.map(|source| Origin { source, label }))
}

/// Lista JSON de tags (`json_group_array` ou a coluna da revisão).
fn tags_at(row: &Row<'_>, index: usize) -> rusqlite::Result<Vec<String>> {
    let json: Option<String> = row.get(index)?;
    match json {
        None => Ok(Vec::new()),
        Some(json) => {
            serde_json::from_str(&json).map_err(|error| invalid(index, error.to_string()))
        }
    }
}

fn revision_from_row(row: &Row<'_>) -> rusqlite::Result<MemoryRevision> {
    Ok(MemoryRevision {
        version: row.get(0)?,
        title: row.get(1)?,
        text: row.get(2)?,
        category: row.get(3)?,
        status: status_at(row, 4)?,
        author: Author {
            app_id: row.get(5)?,
            app: row.get(6)?,
            agent_id: row.get(7)?,
            agent: row.get(8)?,
        },
        reason: row.get(9)?,
        created_at: row.get(10)?,
        title_locked: row.get(11)?,
        tags: tags_at(row, 12)?,
        origin: origin_at(row, 13)?,
    })
}

const REVISION_SELECT: &str = "SELECT r.version, r.title, r.text, r.category, r.status, \
     r.app_id, a.slug, r.agent_id, ga.slug || '/' || g.slug, r.reason, r.created_at, \
     r.title_locked, r.tags, r.origin_source, r.origin_label \
     FROM memory_revisions r \
     LEFT JOIN apps a ON a.id = r.app_id \
     LEFT JOIN agents g ON g.id = r.agent_id \
     LEFT JOIN apps ga ON ga.id = g.app_id";

/// Uma condição `EXISTS` por tag (E lógico), pelo índice `memory_tags (tag, memory_id)`.
pub(super) fn push_tags(conditions: &mut Vec<String>, values: &mut Vec<SqlValue>, tags: &[String]) {
    for tag in tags {
        conditions.push(
            "EXISTS (SELECT 1 FROM memory_tags t WHERE t.memory_id = m.id AND t.tag = ?)"
                .to_owned(),
        );
        values.push(SqlValue::Text(tag.clone()));
    }
}

/// A memória do canal com essa chave.
pub(super) fn find_by_key(
    connection: &Connection,
    channel_id: &str,
    key: &str,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT id FROM memories WHERE channel_id = ?1 AND key = ?2",
            params![channel_id, key],
            |row| row.get(0),
        )
        .optional()
}

/// Troca as tags da memória (a ordem é a posição).
fn write_tags(transaction: &Transaction, id: &str, tags: &[String]) -> Result<(), StoreError> {
    transaction
        .execute("DELETE FROM memory_tags WHERE memory_id = ?1", params![id])
        .map_err(sql_error)?;
    for (position, tag) in tags.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO memory_tags (memory_id, tag, position) VALUES (?1, ?2, ?3)",
                params![id, tag, i64::try_from(position).unwrap_or(i64::MAX)],
            )
            .map_err(sql_error)?;
    }
    Ok(())
}

pub(super) fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

pub(super) fn texts<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<SqlValue> {
    values
        .into_iter()
        .map(|value| SqlValue::Text(value.to_owned()))
        .collect()
}

// ---------- leitura ----------

pub(super) fn get_memory(connection: &Connection, id: &str) -> rusqlite::Result<Option<Memory>> {
    connection
        .query_row(
            &format!("SELECT {MEMORY_COLUMNS} FROM memories m {MEMORY_JOINS} WHERE m.id = ?1"),
            params![id],
            memory_from_row,
        )
        .optional()
}

/// Outra memória do canal com o mesmo `content_hash`.
pub(super) fn find_by_hash(
    connection: &Connection,
    channel_id: &str,
    hash: &[u8],
    exclude: Option<&str>,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT id FROM memories
             WHERE channel_id = ?1 AND content_hash = ?2 AND id <> COALESCE(?3, '')
             ORDER BY created_at, id LIMIT 1",
            params![channel_id, hash, exclude],
            |row| row.get(0),
        )
        .optional()
}

/// De `updated_at` mais recente ao mais antigo; o cursor guarda `(updated_at, id)`.
pub(super) fn list_memories(
    connection: &Connection,
    filter: &MemoryFilter,
    page: &PageRequest,
) -> rusqlite::Result<Page<Memory>> {
    if filter.channel_ids.is_empty() || filter.statuses.is_empty() {
        return Ok(Page {
            items: Vec::new(),
            next: None,
        });
    }
    let mut conditions = vec![
        format!(
            "m.channel_id IN ({})",
            placeholders(filter.channel_ids.len())
        ),
        format!("m.status IN ({})", placeholders(filter.statuses.len())),
    ];
    let mut values = texts(filter.channel_ids.iter().map(String::as_str));
    values.extend(texts(filter.statuses.iter().map(|status| status.as_str())));
    if !filter.categories.is_empty() {
        conditions.push(format!(
            "m.category IN ({})",
            placeholders(filter.categories.len())
        ));
        values.extend(texts(filter.categories.iter().map(String::as_str)));
    }
    if let Some(stale) = filter.stale {
        conditions.push(format!("m.stale_candidate = {}", i64::from(stale)));
    }
    push_tags(&mut conditions, &mut values, &filter.tags);
    if let Some(key) = &filter.key {
        conditions.push("m.key = ?".to_owned());
        values.push(SqlValue::Text(key.clone()));
    }
    for (column, value) in [
        ("m.author_app_id = ?", &filter.author_app_id),
        ("m.author_agent_id = ?", &filter.author_agent_id),
        ("m.updated_at >= ?", &filter.since),
    ] {
        if let Some(value) = value {
            conditions.push(column.to_owned());
            values.push(SqlValue::Text(value.clone()));
        }
    }
    if let Some(after) = &page.after {
        conditions.push("(m.updated_at < ? OR (m.updated_at = ? AND m.id < ?))".to_owned());
        values.extend(texts([
            after.key.as_str(),
            after.key.as_str(),
            after.id.as_str(),
        ]));
    }
    let mut statement = connection.prepare(&format!(
        "SELECT {MEMORY_COLUMNS} FROM memories m {MEMORY_JOINS}
         WHERE {} ORDER BY m.updated_at DESC, m.id DESC LIMIT {}",
        conditions.join(" AND "),
        page.limit + 1
    ))?;
    let mut items = statement
        .query_map(rusqlite::params_from_iter(values), memory_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let next = if items.len() > page.limit as usize {
        items.truncate(page.limit as usize);
        items.last().map(|memory| Cursor {
            key: memory.updated_at.clone(),
            id: memory.id.clone(),
        })
    } else {
        None
    };
    Ok(Page { items, next })
}

/// Da versão mais nova à mais antiga; o cursor guarda a última versão devolvida.
pub(super) fn memory_history(
    connection: &Connection,
    id: &str,
    page: &PageRequest,
) -> rusqlite::Result<Page<MemoryRevision>> {
    let before = page
        .after
        .as_ref()
        .and_then(|after| after.id.parse::<i64>().ok())
        .unwrap_or(i64::MAX);
    let mut statement = connection.prepare(&format!(
        "{REVISION_SELECT} WHERE r.memory_id = ?1 AND r.version < ?2
         ORDER BY r.version DESC LIMIT {}",
        page.limit + 1
    ))?;
    let mut items = statement
        .query_map(params![id, before], revision_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let next = if items.len() > page.limit as usize {
        items.truncate(page.limit as usize);
        items.last().map(|revision| Cursor {
            key: String::new(),
            id: revision.version.to_string(),
        })
    } else {
        None
    };
    Ok(Page { items, next })
}

pub(super) fn get_revision(
    connection: &Connection,
    id: &str,
    version: i64,
) -> rusqlite::Result<Option<MemoryRevision>> {
    connection
        .query_row(
            &format!("{REVISION_SELECT} WHERE r.memory_id = ?1 AND r.version = ?2"),
            params![id, version],
            revision_from_row,
        )
        .optional()
}

/// Expressão FTS5: cada termo entre aspas, unidos por `OR`.
fn match_expression(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// BM25 com peso 2 no título e 1 no texto; empate pelo id.
pub(super) fn search_text(
    connection: &Connection,
    query: &TextQuery,
) -> rusqlite::Result<Vec<Memory>> {
    if query.terms.is_empty() || query.channel_ids.is_empty() || query.statuses.is_empty() {
        return Ok(Vec::new());
    }
    let mut conditions = vec![
        "memory_fts MATCH ?".to_owned(),
        format!(
            "m.channel_id IN ({})",
            placeholders(query.channel_ids.len())
        ),
        format!("m.status IN ({})", placeholders(query.statuses.len())),
    ];
    let mut values = vec![SqlValue::Text(match_expression(&query.terms))];
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
    if !query.only_ids.is_empty() {
        conditions.push(format!("m.id IN ({})", placeholders(query.only_ids.len())));
        values.extend(texts(query.only_ids.iter().map(String::as_str)));
    }
    push_tags(&mut conditions, &mut values, &query.tags);
    let mut statement = connection.prepare(&format!(
        "SELECT {MEMORY_COLUMNS} FROM memory_fts JOIN memories m ON m.seq = memory_fts.rowid
         {MEMORY_JOINS}
         WHERE {} ORDER BY bm25(memory_fts, 2.0, 1.0), m.id LIMIT {}",
        conditions.join(" AND "),
        query.limit
    ))?;
    statement
        .query_map(rusqlite::params_from_iter(values), memory_from_row)?
        .collect()
}

pub(super) fn channel_stats(
    connection: &Connection,
    channel_id: &str,
) -> rusqlite::Result<Option<ChannelStats>> {
    let Some((channel, version)) = connection
        .query_row(
            "SELECT slug, version FROM channels WHERE id = ?1",
            params![channel_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut stats = connection.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(status = 'active'), 0),
                COALESCE(SUM(status = 'permanent'), 0),
                COALESCE(SUM(status = 'archived'), 0),
                COALESCE(SUM(title_state = 'pending'), 0),
                COALESCE(SUM(embedding_state = 'pending'), 0),
                COALESCE(SUM(retrieval_count), 0),
                MAX(updated_at),
                COALESCE(SUM(stale_candidate), 0)
         FROM memories WHERE channel_id = ?1",
        params![channel_id],
        |row| {
            let count = |index: usize| row.get::<_, i64>(index).map(|n| n.max(0) as u64);
            Ok(ChannelStats {
                channel: channel.clone(),
                channel_id: channel_id.to_owned(),
                version,
                memories: MemoryCounts {
                    total: count(0)?,
                    active: count(1)?,
                    permanent: count(2)?,
                    archived: count(3)?,
                },
                by_category: BTreeMap::new(),
                pending: PendingCounts {
                    title: count(4)?,
                    embedding: count(5)?,
                },
                retrievals: count(6)?,
                last_memory_at: row.get(7)?,
                stale: count(8)?,
            })
        },
    )?;
    let mut statement = connection.prepare(
        "SELECT category, COUNT(*) FROM memories WHERE channel_id = ?1 GROUP BY category",
    )?;
    stats.by_category = statement
        .query_map(params![channel_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?.max(0) as u64,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(stats))
}

/// Memórias por categoria, em toda a rede.
pub(super) fn category_counts(connection: &Connection) -> rusqlite::Result<Vec<(String, u64)>> {
    let mut statement = connection
        .prepare("SELECT category, COUNT(*) FROM memories GROUP BY category ORDER BY category")?;
    statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?.max(0) as u64,
            ))
        })?
        .collect()
}

/// Categorias usadas fora de `retained` (guarda de `CATEGORY_IN_USE`).
pub(super) fn categories_in_use_outside(
    transaction: &Transaction,
    retained: &[String],
) -> Result<bool, StoreError> {
    let sql = if retained.is_empty() {
        "SELECT EXISTS (SELECT 1 FROM memories)".to_owned()
    } else {
        format!(
            "SELECT EXISTS (SELECT 1 FROM memories WHERE category NOT IN ({}))",
            placeholders(retained.len())
        )
    };
    transaction
        .query_row(
            &sql,
            rusqlite::params_from_iter(retained.iter().map(String::as_str)),
            |row| row.get(0),
        )
        .map_err(sql_error)
}

// ---------- escrita ----------

fn load(transaction: &Transaction, id: &str) -> Result<Memory, StoreError> {
    get_memory(transaction, id)
        .map_err(sql_error)?
        .ok_or(StoreError::NotFound)
}

/// A memória alvo, se ainda estiver no canal conferido pelo `core`.
fn load_target(transaction: &Transaction, target: &MemoryTarget) -> Result<Memory, StoreError> {
    let memory = load(transaction, &target.id)?;
    if memory.channel_id != target.channel_id {
        return Err(StoreError::NotFound);
    }
    Ok(memory)
}

/// Canal arquivado não recebe escrita (Q-006).
fn ensure_channel_active(transaction: &Transaction, channel_id: &str) -> Result<(), StoreError> {
    let archived: Option<Option<String>> = transaction
        .query_row(
            "SELECT archived_at FROM channels WHERE id = ?1",
            params![channel_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    match archived {
        None => Err(StoreError::Conflict("CHANNEL_NOT_FOUND")),
        Some(Some(_)) => Err(StoreError::Conflict("CHANNEL_ARCHIVED")),
        Some(None) => Ok(()),
    }
}

pub(super) fn bump_channel(transaction: &Transaction, channel_id: &str) -> Result<(), StoreError> {
    transaction
        .execute(
            "UPDATE channels SET version = version + 1 WHERE id = ?1",
            params![channel_id],
        )
        .map_err(sql_error)?;
    promote_importance(transaction, channel_id)
}

/// A nota do consolidador só passa a valer na transação que incrementa `channels.version`
/// (009 D-001): a ordem do contexto nunca muda sem mudar a `version`. Toda escrita que
/// incrementa `channels.version` chama esta função.
pub(super) fn promote_importance(
    transaction: &Transaction,
    channel_id: &str,
) -> Result<(), StoreError> {
    transaction
        .execute(
            "UPDATE memories SET importance = importance_pending, importance_pending = NULL
             WHERE channel_id = ?1 AND importance_pending IS NOT NULL",
            params![channel_id],
        )
        .map_err(sql_error)?;
    Ok(())
}

fn revision(
    transaction: &Transaction,
    memory: &Memory,
    reason: &str,
    context: &WriteContext,
) -> Result<(), StoreError> {
    revision_with(transaction, memory, reason, None, context)
}

/// Revisão com o estado atual da memória (título, trava e tags) e a origem informada.
fn revision_with(
    transaction: &Transaction,
    memory: &Memory,
    reason: &str,
    origin: Option<&Origin>,
    context: &WriteContext,
) -> Result<(), StoreError> {
    transaction
        .execute(
            "INSERT INTO memory_revisions
                 (memory_id, version, title, text, category, status, app_id, agent_id, reason,
                  created_at, title_locked, tags, origin_source, origin_label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                memory.id,
                memory.version,
                memory.title,
                memory.text,
                memory.category,
                memory.status.as_str(),
                context.actor.app_id.as_deref().unwrap_or(SYSTEM_APP),
                context.actor.agent_id,
                reason,
                context.at,
                memory.title_locked,
                tags_json(&memory.tags)?,
                origin.map(|origin| origin.source.as_str()),
                origin.and_then(|origin| origin.label.as_deref())
            ],
        )
        .map_err(sql_error)?;
    Ok(())
}

fn tags_json(tags: &[String]) -> Result<String, StoreError> {
    serde_json::to_string(tags).map_err(|error| StoreError::Internal(error.to_string()))
}

/// Grava a origem da última edição.
fn set_last_origin(transaction: &Transaction, id: &str, origin: &Origin) -> Result<(), StoreError> {
    transaction
        .execute(
            "UPDATE memories SET last_origin_source = ?1, last_origin_label = ?2 WHERE id = ?3",
            params![origin.source, origin.label, id],
        )
        .map_err(sql_error)?;
    Ok(())
}

/// `pending` quando a escrita enfileira o `embed`; `off` com os embeddings desligados.
fn embedding_state(jobs: &[NewJob]) -> &'static str {
    if jobs.iter().any(|job| job.kind == "embed") {
        "pending"
    } else {
        "off"
    }
}

fn enqueue(
    transaction: &Transaction,
    jobs: &[NewJob],
    memory: &Memory,
    context: &WriteContext,
) -> Result<(), StoreError> {
    for job in jobs {
        let mut payload = job.extra.clone();
        payload.insert("memory_id".to_owned(), json!(memory.id));
        payload.insert("version".to_owned(), json!(memory.version));
        transaction
            .execute(
                "INSERT INTO jobs (kind, payload_json, run_after, created_at)
                 VALUES (?1, ?2, ?3, ?3)",
                params![job.kind, Value::Object(payload).to_string(), context.at],
            )
            .map_err(sql_error)?;
    }
    Ok(())
}

/// O que a auditoria guarda de uma memória: metadados e o hash, nunca o texto nem o título.
fn audited(transaction: &Transaction, id: &str) -> Result<Value, StoreError> {
    transaction
        .query_row(
            "SELECT id, channel_id, category, status, version, content_hash, title_state,
                    embedding_state, origin_source
             FROM memories WHERE id = ?1",
            params![id],
            |row| {
                let hash: Vec<u8> = row.get(5)?;
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "channel_id": row.get::<_, String>(1)?,
                    "category": row.get::<_, String>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "version": row.get::<_, i64>(4)?,
                    "content_hash": hash_hex(&hash),
                    "title_state": row.get::<_, String>(6)?,
                    "embedding_state": row.get::<_, String>(7)?,
                    "origin_source": row.get::<_, Option<String>>(8)?,
                }))
            },
        )
        .map_err(sql_error)
}

fn audit(
    transaction: &Transaction,
    context: &WriteContext,
    action: &str,
    id: &str,
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), StoreError> {
    journal::audit(
        transaction,
        context,
        Entry {
            action,
            entity: "memory",
            entity_id: Some(id),
            before,
            after,
        },
    )
}

pub(super) fn create_memory(
    transaction: &Transaction,
    new: &NewMemory,
    context: &WriteContext,
) -> Result<MemoryWrite, StoreError> {
    ensure_channel_active(transaction, &new.channel_id)?;
    // Chave natural (0.11.0): existente → `MEMORY_KEY_EXISTS` ou, com `upsert`, uma edição.
    if let Some(key) = &new.key
        && let Some(existing) = find_by_key(transaction, &new.channel_id, key).map_err(sql_error)?
    {
        if !new.upsert {
            return Err(StoreError::Conflict("MEMORY_KEY_EXISTS"));
        }
        let before = load(transaction, &existing)?;
        let mut similar = new.similar.clone();
        similar.retain(|item| item.id != existing);
        let patch = MemoryPatch {
            expected_version: None,
            content: Some(MemoryContent {
                title: new.title.clone(),
                text: new.text.clone(),
                content_hash: new.content_hash.clone(),
            }),
            category: Some(new.category.clone()),
            title: new.title_locked.then(|| TitleChange {
                title: Some(new.title.clone()),
                locked: true,
            }),
            tags: new.tags.clone(),
            origin: new.origin.clone(),
            dedupe: new.dedupe,
            // Upsert reafirma o fato: a memória arquivada volta ao status pedido (decisão 0032).
            restore: Some(new.status),
            reason: "update",
            jobs: new.jobs.clone(),
            similar,
        };
        let write = patch_memory(transaction, before, &patch, context)?;
        journal::remember_as(transaction, context, &write, 200)?;
        return Ok(write);
    }
    if new.dedupe
        && new.key.is_none()
        && let Some(existing) = find_by_hash(transaction, &new.channel_id, &new.content_hash, None)
            .map_err(sql_error)?
    {
        let write = MemoryWrite {
            memory: load(transaction, &existing)?,
            created: false,
            duplicate: true,
            similar: new.similar.clone(),
        };
        journal::remember_as(transaction, context, &write, 200)?;
        return Ok(write);
    }
    let id = free_id(transaction, "memories", ids::MEMORY_PREFIX)?;
    let (origin_source, origin_label) = match &new.origin {
        Some(origin) => (Some(origin.source.as_str()), origin.label.as_deref()),
        None => (None, None),
    };
    transaction
        .execute(
            "INSERT INTO memories
                 (id, channel_id, category, title, text, status, content_hash, version,
                  title_state, embedding_state, author_app_id, author_agent_id, origin_source,
                  origin_label, created_at, updated_at, title_locked, key, last_origin_source,
                  last_origin_label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?14, ?13, ?8, ?9, ?10, ?11,
                     ?12, ?12, ?15, ?16, ?10, ?11)",
            params![
                id,
                new.channel_id,
                new.category,
                new.title,
                new.text,
                new.status.as_str(),
                new.content_hash,
                context.actor.app_id.as_deref().unwrap_or(SYSTEM_APP),
                context.actor.agent_id,
                origin_source,
                origin_label,
                context.at,
                embedding_state(&new.jobs),
                if new.title_locked { "ready" } else { "pending" },
                new.title_locked,
                new.key
            ],
        )
        .map_err(|error| super::unique_conflict(error, "MEMORY_KEY_EXISTS"))?;
    if let Some(tags) = &new.tags {
        write_tags(transaction, &id, tags)?;
    }
    let memory = load(transaction, &id)?;
    revision_with(transaction, &memory, "create", new.origin.as_ref(), context)?;
    bump_channel(transaction, &new.channel_id)?;
    audit(
        transaction,
        context,
        "memory.create",
        &id,
        None,
        Some(audited(transaction, &id)?),
    )?;
    enqueue(transaction, &new.jobs, &memory, context)?;
    journal::emit(transaction, context, std::slice::from_ref(&memory.id))?;
    let write = MemoryWrite {
        memory,
        created: true,
        duplicate: false,
        similar: new.similar.clone(),
    };
    journal::remember(transaction, context, &write)?;
    Ok(write)
}

/// Importação em lote (008 FR-009): `created_at`, autor e origem preservados; a revisão 1 tem
/// o mesmo autor e instante. Uma auditoria `memory.import` com os ids (sem textos) e um
/// evento com todos os ids criados.
pub(super) fn import_memories(
    transaction: &Transaction,
    channel_id: &str,
    items: &[NewImportedMemory],
    dedupe: bool,
    context: &WriteContext,
) -> Result<ImportResult, StoreError> {
    ensure_channel_active(transaction, channel_id)?;
    let mut result = ImportResult {
        created: Vec::new(),
        duplicates: Vec::new(),
        updated: Vec::new(),
    };
    let mut changed: Vec<String> = Vec::new();
    for item in items {
        // Chave existente no canal: o item vira uma edição (upsert), com o token como autor.
        if let Some(key) = &item.key
            && let Some(existing) = find_by_key(transaction, channel_id, key).map_err(sql_error)?
        {
            let before = load(transaction, &existing)?;
            let patch = MemoryPatch {
                expected_version: None,
                content: Some(MemoryContent {
                    title: item.title.clone(),
                    text: item.text.clone(),
                    content_hash: item.content_hash.clone(),
                }),
                category: Some(item.category.clone()),
                title: item.title_locked.then(|| TitleChange {
                    title: Some(item.title.clone()),
                    locked: true,
                }),
                tags: item.tags.clone(),
                origin: item.origin.clone(),
                dedupe,
                // Como o upsert de `create_memory`: a arquivada volta ao status do item.
                restore: (item.status != MemoryStatus::Archived).then_some(item.status),
                reason: "update",
                jobs: item.jobs.clone(),
                similar: Vec::new(),
            };
            let write = patch_memory_quiet(transaction, before, &patch, context)?;
            if write.0 {
                changed.push(existing.clone());
            }
            result.updated.push(ImportCreated {
                index: item.index,
                id: existing,
            });
            continue;
        }
        if dedupe
            && item.key.is_none()
            && let Some(existing) = find_by_hash(transaction, channel_id, &item.content_hash, None)
                .map_err(sql_error)?
        {
            result.duplicates.push(ImportDuplicate {
                index: item.index,
                existing_id: existing,
            });
            continue;
        }
        let id = free_id(transaction, "memories", ids::MEMORY_PREFIX)?;
        let (origin_source, origin_label) = match &item.origin {
            Some(origin) => (Some(origin.source.as_str()), origin.label.as_deref()),
            None => (None, None),
        };
        transaction
            .execute(
                "INSERT INTO memories
                     (id, channel_id, category, title, text, status, content_hash, version,
                      title_state, embedding_state, author_app_id, author_agent_id,
                      origin_source, origin_label, created_at, updated_at, title_locked, key,
                      last_origin_source, last_origin_label)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?14, ?13, ?8, ?9, ?10, ?11,
                         ?12, ?12, ?15, ?16, ?10, ?11)",
                params![
                    id,
                    channel_id,
                    item.category,
                    item.title,
                    item.text,
                    item.status.as_str(),
                    item.content_hash,
                    item.author_app_id,
                    item.author_agent_id,
                    origin_source,
                    origin_label,
                    item.created_at,
                    embedding_state(&item.jobs),
                    if item.title_locked {
                        "ready"
                    } else {
                        "pending"
                    },
                    item.title_locked,
                    item.key
                ],
            )
            .map_err(|error| super::unique_conflict(error, "MEMORY_KEY_EXISTS"))?;
        if let Some(tags) = &item.tags {
            write_tags(transaction, &id, tags)?;
        }
        let memory = load(transaction, &id)?;
        transaction
            .execute(
                "INSERT INTO memory_revisions
                     (memory_id, version, title, text, category, status, app_id, agent_id,
                      reason, created_at, title_locked, tags, origin_source, origin_label)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, 'import', ?8, ?9, ?10, ?11, ?12)",
                params![
                    memory.id,
                    memory.title,
                    memory.text,
                    memory.category,
                    memory.status.as_str(),
                    item.author_app_id,
                    item.author_agent_id,
                    item.created_at,
                    memory.title_locked,
                    tags_json(&memory.tags)?,
                    origin_source,
                    origin_label
                ],
            )
            .map_err(sql_error)?;
        enqueue(transaction, &item.jobs, &memory, context)?;
        result.created.push(ImportCreated {
            index: item.index,
            id,
        });
    }
    let created: Vec<String> = result.created.iter().map(|item| item.id.clone()).collect();
    if !created.is_empty() || !changed.is_empty() {
        bump_channel(transaction, channel_id)?;
        journal::audit(
            transaction,
            context,
            Entry {
                action: "memory.import",
                entity: "channel",
                entity_id: Some(channel_id),
                before: None,
                after: Some(json!({
                    "created": created,
                    "updated": changed,
                    "duplicates": result.duplicates.len(),
                })),
            },
        )?;
        let mut ids = created;
        ids.extend(changed);
        journal::emit(transaction, context, &ids)?;
    }
    journal::remember(transaction, context, &result)?;
    Ok(result)
}

pub(super) fn update_memory(
    transaction: &Transaction,
    target: &MemoryTarget,
    patch: &MemoryPatch,
    context: &WriteContext,
) -> Result<MemoryWrite, StoreError> {
    let before = load_target(transaction, target)?;
    if patch
        .expected_version
        .is_some_and(|expected| expected != before.version)
    {
        return Err(StoreError::Conflict("MEMORY_VERSION_CONFLICT"));
    }
    let write = patch_memory(transaction, before, patch, context)?;
    journal::remember(transaction, context, &write)?;
    Ok(write)
}

/// Aplica a edição (sem a idempotência): sem mudança efetiva, nada é gravado (Q-007). O
/// texto novo volta o título a provisório, salvo título travado; título, trava e tags mudam
/// sem tocar o `content_hash`; `origin` só vale com alguma mudança.
fn patch_memory(
    transaction: &Transaction,
    before: Memory,
    patch: &MemoryPatch,
    context: &WriteContext,
) -> Result<MemoryWrite, StoreError> {
    ensure_channel_active(transaction, &before.channel_id)?;
    let content = patch
        .content
        .as_ref()
        .filter(|content| content.text != before.text);
    let category = patch
        .category
        .as_ref()
        .filter(|category| **category != before.category);
    let title = patch.title.as_ref().filter(|change| {
        change.locked != before.title_locked
            || change
                .title
                .as_ref()
                .is_some_and(|title| *title != before.title)
    });
    let tags = patch.tags.as_ref().filter(|tags| **tags != before.tags);
    let restore = patch
        .restore
        .filter(|_| before.status == MemoryStatus::Archived);
    if content.is_none()
        && category.is_none()
        && title.is_none()
        && tags.is_none()
        && restore.is_none()
    {
        return Ok(MemoryWrite {
            memory: before,
            created: false,
            duplicate: false,
            similar: patch.similar.clone(),
        });
    }
    if let Some(content) = content
        && patch.dedupe
        && find_by_hash(
            transaction,
            &before.channel_id,
            &content.content_hash,
            Some(&before.id),
        )
        .map_err(sql_error)?
        .is_some()
    {
        return Err(StoreError::Conflict("MEMORY_DUPLICATE"));
    }
    let audit_before = audited(transaction, &before.id)?;
    let locked_after = title.map_or(before.title_locked, |change| change.locked);
    if let Some(content) = content {
        transaction
            .execute(
                "UPDATE memories SET text = ?1, content_hash = ?2, embedding_state = ?4,
                     importance = NULL, importance_pending = NULL, stale_candidate = 0
                 WHERE id = ?3",
                params![
                    content.text,
                    content.content_hash,
                    before.id,
                    embedding_state(&patch.jobs)
                ],
            )
            .map_err(sql_error)?;
        if !locked_after {
            // Título provisório até o Redator: só quando não está (nem fica) travado.
            transaction
                .execute(
                    "UPDATE memories SET title = ?1, title_state = 'pending' WHERE id = ?2",
                    params![content.title, before.id],
                )
                .map_err(sql_error)?;
        }
    }
    if let Some(change) = title {
        if let Some(title) = &change.title {
            transaction
                .execute(
                    "UPDATE memories SET title = ?1 WHERE id = ?2",
                    params![title, before.id],
                )
                .map_err(sql_error)?;
        }
        let state = if change.locked { "ready" } else { "pending" };
        transaction
            .execute(
                "UPDATE memories SET title_locked = ?1, title_state = ?2 WHERE id = ?3",
                params![change.locked, state, before.id],
            )
            .map_err(sql_error)?;
    }
    if let Some(category) = category {
        transaction
            .execute(
                "UPDATE memories SET category = ?1 WHERE id = ?2",
                params![category, before.id],
            )
            .map_err(sql_error)?;
    }
    if let Some(tags) = tags {
        write_tags(transaction, &before.id, tags)?;
    }
    if let Some(status) = restore {
        // Como `restore_memory`: deixa de ser fundida e volta a ser usada.
        transaction
            .execute(
                "UPDATE memories SET status = ?1, archived_at = NULL, merged_into = NULL,
                     stale_candidate = 0
                 WHERE id = ?2",
                params![status.as_str(), before.id],
            )
            .map_err(sql_error)?;
    }
    if let Some(origin) = &patch.origin {
        set_last_origin(transaction, &before.id, origin)?;
    }
    transaction
        .execute(
            "UPDATE memories SET version = version + 1, updated_at = ?1 WHERE id = ?2",
            params![context.at, before.id],
        )
        .map_err(sql_error)?;
    let after = load(transaction, &before.id)?;
    revision_with(
        transaction,
        &after,
        patch.reason,
        patch.origin.as_ref(),
        context,
    )?;
    bump_channel(transaction, &after.channel_id)?;
    audit(
        transaction,
        context,
        &format!("memory.{}", patch.reason),
        &after.id,
        Some(audit_before),
        Some(audited(transaction, &after.id)?),
    )?;
    let unlocked = title.is_some_and(|change| !change.locked);
    if content.is_some() || unlocked {
        enqueue(transaction, &patch.jobs, &after, context)?;
    }
    journal::emit(transaction, context, std::slice::from_ref(&after.id))?;
    Ok(MemoryWrite {
        memory: after,
        created: false,
        duplicate: false,
        similar: patch.similar.clone(),
    })
}

/// `patch_memory` sem o evento por memória (a importação emite um só, com todos os ids);
/// devolve se algo mudou.
fn patch_memory_quiet(
    transaction: &Transaction,
    before: Memory,
    patch: &MemoryPatch,
    context: &WriteContext,
) -> Result<(bool, MemoryWrite), StoreError> {
    let version = before.version;
    let quiet = WriteContext {
        events: Vec::new(),
        idempotency: None,
        ..context.clone()
    };
    let write = patch_memory(transaction, before, patch, &quiet)?;
    Ok((write.memory.version != version, write))
}

pub(super) fn set_memory_status(
    transaction: &Transaction,
    target: &MemoryTarget,
    action: StatusAction,
    context: &WriteContext,
) -> Result<Memory, StoreError> {
    let before = load_target(transaction, target)?;
    ensure_channel_active(transaction, &before.channel_id)?;
    let next = match (action, before.status) {
        (StatusAction::Archive, MemoryStatus::Archived)
        | (StatusAction::Restore, MemoryStatus::Active | MemoryStatus::Permanent)
        | (StatusAction::Pin, MemoryStatus::Permanent)
        | (StatusAction::Unpin, MemoryStatus::Active | MemoryStatus::Archived) => None,
        (StatusAction::Archive, _) => Some(MemoryStatus::Archived),
        (StatusAction::Restore | StatusAction::Unpin, _) => Some(MemoryStatus::Active),
        (StatusAction::Pin, _) => Some(MemoryStatus::Permanent),
    };
    let Some(next) = next else {
        journal::remember(transaction, context, &before)?;
        return Ok(before);
    };
    let audit_before = audited(transaction, &before.id)?;
    let archived_at = (next == MemoryStatus::Archived).then_some(context.at.as_str());
    transaction
        .execute(
            "UPDATE memories SET status = ?1, archived_at = ?2, version = version + 1,
                 updated_at = ?3
             WHERE id = ?4",
            params![next.as_str(), archived_at, context.at, before.id],
        )
        .map_err(sql_error)?;
    if action == StatusAction::Restore {
        // A memória restaurada deixa de ser fundida e volta a ser usada (009 Q-002).
        transaction
            .execute(
                "UPDATE memories SET merged_into = NULL, stale_candidate = 0 WHERE id = ?1",
                params![before.id],
            )
            .map_err(sql_error)?;
    }
    let after = load(transaction, &before.id)?;
    revision(transaction, &after, action.reason(), context)?;
    bump_channel(transaction, &after.channel_id)?;
    audit(
        transaction,
        context,
        &format!("memory.{}", action.reason()),
        &after.id,
        Some(audit_before),
        Some(audited(transaction, &after.id)?),
    )?;
    journal::emit(transaction, context, std::slice::from_ref(&after.id))?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

pub(super) fn move_memory(
    transaction: &Transaction,
    target: &MemoryTarget,
    to_channel_id: &str,
    dedupe: bool,
    context: &WriteContext,
) -> Result<Memory, StoreError> {
    let before = load_target(transaction, target)?;
    ensure_channel_active(transaction, &before.channel_id)?;
    ensure_channel_active(transaction, to_channel_id)?;
    if before.channel_id == to_channel_id {
        journal::remember(transaction, context, &before)?;
        return Ok(before);
    }
    if dedupe {
        let duplicate: bool = transaction
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM memories
                     WHERE channel_id = ?1 AND id <> ?2
                       AND content_hash = (SELECT content_hash FROM memories WHERE id = ?2))",
                params![to_channel_id, before.id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if duplicate {
            return Err(StoreError::Conflict("MEMORY_DUPLICATE"));
        }
    }
    // A chave é única por canal: a memória leva a chave e colide com a do destino.
    if let Some(key) = &before.key
        && find_by_key(transaction, to_channel_id, key)
            .map_err(sql_error)?
            .is_some()
    {
        return Err(StoreError::Conflict("MEMORY_KEY_EXISTS"));
    }
    let audit_before = audited(transaction, &before.id)?;
    transaction
        .execute(
            "UPDATE memories SET channel_id = ?1, version = version + 1, updated_at = ?2
             WHERE id = ?3",
            params![to_channel_id, context.at, before.id],
        )
        .map_err(|error| super::unique_conflict(error, "MEMORY_KEY_EXISTS"))?;
    let after = load(transaction, &before.id)?;
    revision(transaction, &after, "move", context)?;
    bump_channel(transaction, &before.channel_id)?;
    bump_channel(transaction, to_channel_id)?;
    audit(
        transaction,
        context,
        "memory.move",
        &after.id,
        Some(audit_before),
        Some(audited(transaction, &after.id)?),
    )?;
    journal::emit(transaction, context, std::slice::from_ref(&after.id))?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

/// Remove a memória (o FTS sai pelo trigger) e as revisões, menos a última.
pub(super) fn delete_memory(
    transaction: &Transaction,
    target: &MemoryTarget,
    context: &WriteContext,
) -> Result<Memory, StoreError> {
    let before = load_target(transaction, target)?;
    let audit_before = audited(transaction, &before.id)?;
    transaction
        .execute(
            "DELETE FROM memory_revisions WHERE memory_id = ?1 AND version <
                 (SELECT MAX(version) FROM memory_revisions WHERE memory_id = ?1)",
            params![before.id],
        )
        .map_err(sql_error)?;
    transaction
        .execute("DELETE FROM memories WHERE id = ?1", params![before.id])
        .map_err(sql_error)?;
    bump_channel(transaction, &before.channel_id)?;
    audit(
        transaction,
        context,
        "memory.delete",
        &before.id,
        Some(audit_before),
        None,
    )?;
    journal::emit(transaction, context, std::slice::from_ref(&before.id))?;
    journal::remember(transaction, context, &before)?;
    Ok(before)
}

/// Soma os contadores sem mexer em `version` nem em `updated_at`.
pub(super) fn bump_retrievals(
    transaction: &Transaction,
    batch: &[RetrievalBump],
) -> rusqlite::Result<()> {
    let mut statement = transaction.prepare(
        "UPDATE memories SET retrieval_count = retrieval_count + ?2,
             last_retrieved_at = CASE
                 WHEN last_retrieved_at IS NULL OR last_retrieved_at < ?3 THEN ?3
                 ELSE last_retrieved_at END
         WHERE id = ?1",
    )?;
    for bump in batch {
        statement.execute(params![bump.memory_id, bump.count, bump.at])?;
    }
    Ok(())
}

// ---------- Redator (006) ----------

/// `content_hash` atual da memória.
pub(super) fn content_hash(connection: &Connection, id: &str) -> rusqlite::Result<Option<Vec<u8>>> {
    connection
        .query_row(
            "SELECT content_hash FROM memories WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
}

/// Título (e texto) do Redator. O título não cria revisão nem muda `memories.version`
/// (decisão 0016): só `title_state = ready`, `channels.version + 1` e a auditoria. O texto
/// revisado é uma revisão `system/redator` com `version + 1` e um job `embed`.
pub(super) fn apply_redaction(
    transaction: &Transaction,
    redaction: &Redaction,
    context: &WriteContext,
) -> Result<Memory, StoreError> {
    let current = content_hash(transaction, &redaction.memory_id)
        .map_err(sql_error)?
        .ok_or(StoreError::NotFound)?;
    if current != redaction.expected_hash {
        return Err(StoreError::Conflict("MEMORY_CHANGED"));
    }
    let audit_before = audited(transaction, &redaction.memory_id)?;
    // Título travado (do cliente) nunca é trocado pelo Redator (0.11.0).
    if let Some(title) = &redaction.title {
        transaction
            .execute(
                "UPDATE memories SET title = ?1, title_state = 'ready'
                 WHERE id = ?2 AND title_locked = 0",
                params![title, redaction.memory_id],
            )
            .map_err(sql_error)?;
    }
    let state = if redaction.embed { "pending" } else { "off" };
    if let Some((text, hash)) = &redaction.text {
        transaction
            .execute(
                "UPDATE memories SET text = ?1, content_hash = ?2, embedding_state = ?5,
                     importance = NULL, importance_pending = NULL, version = version + 1,
                     updated_at = ?3
                 WHERE id = ?4",
                params![text, hash, context.at, redaction.memory_id, state],
            )
            .map_err(sql_error)?;
    } else if redaction.embed {
        // O vetor é de `título + texto`: o título novo também revetoriza.
        transaction
            .execute(
                "UPDATE memories SET embedding_state = 'pending' WHERE id = ?1",
                params![redaction.memory_id],
            )
            .map_err(sql_error)?;
    }
    let after = load(transaction, &redaction.memory_id)?;
    if redaction.text.is_some() {
        revision(transaction, &after, "redact", context)?;
    }
    if redaction.embed {
        enqueue(
            transaction,
            &[NewJob {
                kind: "embed",
                extra: serde_json::Map::new(),
            }],
            &after,
            context,
        )?;
    }
    bump_channel(transaction, &after.channel_id)?;
    audit(
        transaction,
        context,
        "memory.redact",
        &after.id,
        Some(audit_before),
        Some(audited(transaction, &after.id)?),
    )?;
    journal::emit(transaction, context, std::slice::from_ref(&after.id))?;
    Ok(after)
}

/// `title_state = fallback` só para o conteúdo esperado com título ainda provisório.
pub(super) fn set_title_fallback(
    transaction: &Transaction,
    memory_id: &str,
    expected_hash: &[u8],
) -> rusqlite::Result<bool> {
    let changed = transaction.execute(
        "UPDATE memories SET title_state = 'fallback'
         WHERE id = ?1 AND content_hash = ?2 AND title_state = 'pending' AND title_locked = 0",
        params![memory_id, expected_hash],
    )?;
    Ok(changed > 0)
}

// ---------- consolidação (009) ----------

/// Fusão do consolidador numa transação: confere os dois `content_hash` e o status `active`
/// (senão `Conflict("MEMORY_CHANGED")`), grava o texto e os contadores na mantida (revisão
/// `consolidation.merge` quando o texto muda) e arquiva a outra com `merged_into`.
pub(super) fn merge_memories(
    transaction: &Transaction,
    merge: &MemoryMerge,
    context: &WriteContext,
) -> Result<(), StoreError> {
    let keep = load_target(
        transaction,
        &MemoryTarget {
            id: merge.keep_id.clone(),
            channel_id: merge.channel_id.clone(),
        },
    )?;
    let other = load_target(
        transaction,
        &MemoryTarget {
            id: merge.archive_id.clone(),
            channel_id: merge.channel_id.clone(),
        },
    )?;
    ensure_channel_active(transaction, &merge.channel_id)?;
    let current = |id: &str| -> Result<Vec<u8>, StoreError> {
        content_hash(transaction, id)
            .map_err(sql_error)?
            .ok_or(StoreError::NotFound)
    };
    if keep.status != MemoryStatus::Active
        || other.status != MemoryStatus::Active
        || current(&keep.id)? != merge.keep_hash
        || current(&other.id)? != merge.archive_hash
    {
        return Err(StoreError::Conflict("MEMORY_CHANGED"));
    }
    if let Some((_, hash, _)) = &merge.text {
        let duplicate: bool = transaction
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM memories
                     WHERE channel_id = ?1 AND content_hash = ?2 AND id NOT IN (?3, ?4))",
                params![merge.channel_id, hash, keep.id, other.id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if duplicate {
            return Err(StoreError::Conflict("MEMORY_DUPLICATE"));
        }
    }
    let keep_before = audited(transaction, &keep.id)?;
    let other_before = audited(transaction, &other.id)?;
    // Contadores somados e o uso mais recente das duas: a mantida representa as duas.
    let latest = |left: &Option<String>, right: &Option<String>| match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right).clone()),
        (left, right) => left.clone().or_else(|| right.clone()),
    };
    let (keep_injected_at, other_injected_at): (Option<String>, Option<String>) = transaction
        .query_row(
            "SELECT (SELECT last_injected_at FROM memories WHERE id = ?1),
                    (SELECT last_injected_at FROM memories WHERE id = ?2)",
            params![keep.id, other.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE memories SET retrieval_count = retrieval_count + ?2,
                 injected_count = injected_count + ?3, last_retrieved_at = ?4,
                 last_injected_at = ?5, stale_candidate = 0
             WHERE id = ?1",
            params![
                keep.id,
                other.retrieval_count,
                other.injected_count,
                latest(&keep.last_retrieved_at, &other.last_retrieved_at),
                latest(&keep_injected_at, &other_injected_at)
            ],
        )
        .map_err(sql_error)?;
    if let Some((text, hash, title)) = &merge.text {
        transaction
            .execute(
                "UPDATE memories SET text = ?1, content_hash = ?2,
                     title = CASE WHEN title_locked = 1 THEN title ELSE ?3 END,
                     title_state = CASE WHEN title_locked = 1 THEN title_state ELSE 'pending' END,
                     embedding_state = ?4, importance = NULL,
                     importance_pending = NULL, version = version + 1, updated_at = ?5
                 WHERE id = ?6",
                params![
                    text,
                    hash,
                    title,
                    embedding_state(&merge.jobs),
                    context.at,
                    keep.id
                ],
            )
            .map_err(sql_error)?;
        let after = load(transaction, &keep.id)?;
        revision(transaction, &after, "consolidation.merge", context)?;
        enqueue(transaction, &merge.jobs, &after, context)?;
    }
    transaction
        .execute(
            "UPDATE memories SET status = 'archived', archived_at = ?1, merged_into = ?2,
                 stale_candidate = 0, version = version + 1, updated_at = ?1
             WHERE id = ?3",
            params![context.at, keep.id, other.id],
        )
        .map_err(sql_error)?;
    let archived = load(transaction, &other.id)?;
    revision(transaction, &archived, "consolidation.merge", context)?;
    bump_channel(transaction, &merge.channel_id)?;
    let mut keep_after = audited(transaction, &keep.id)?;
    keep_after["merged"] = json!(other.id);
    audit(
        transaction,
        context,
        "memory.merge",
        &keep.id,
        Some(keep_before),
        Some(keep_after),
    )?;
    let mut other_after = audited(transaction, &other.id)?;
    other_after["reason"] = json!("merged");
    other_after["merged_into"] = json!(keep.id);
    audit(
        transaction,
        context,
        "memory.archive",
        &other.id,
        Some(other_before),
        Some(other_after),
    )?;
    journal::emit(transaction, context, &[])?;
    Ok(())
}

/// Desuso: sinaliza e limpa sem mudar `version`; arquiva com revisão `consolidation.stale`,
/// auditoria, `channels.version + 1` e o evento (só as que ainda estão `active` com o mesmo
/// conteúdo).
pub(super) fn apply_stale(
    transaction: &Transaction,
    write: &StaleWrite,
    context: &WriteContext,
) -> Result<StaleOutcome, StoreError> {
    let mut outcome = StaleOutcome::default();
    for (ids, value) in [(&write.flag, 1), (&write.clear, 0)] {
        for id in ids {
            let changed = transaction
                .execute(
                    "UPDATE memories SET stale_candidate = ?1
                     WHERE id = ?2 AND channel_id = ?3 AND status = 'active'
                       AND stale_candidate <> ?1",
                    params![value, id, write.channel_id],
                )
                .map_err(sql_error)? as u64;
            if value == 1 {
                outcome.flagged += changed;
            } else {
                outcome.cleared += changed;
            }
        }
    }
    if write.archive.is_empty() {
        return Ok(outcome);
    }
    ensure_channel_active(transaction, &write.channel_id)?;
    for (id, hash) in &write.archive {
        let before = audited(transaction, id);
        let changed = transaction
            .execute(
                "UPDATE memories SET status = 'archived', archived_at = ?1, stale_candidate = 1,
                     version = version + 1, updated_at = ?1
                 WHERE id = ?2 AND channel_id = ?3 AND status = 'active' AND content_hash = ?4",
                params![context.at, id, write.channel_id, hash],
            )
            .map_err(sql_error)?;
        if changed == 0 {
            continue;
        }
        let after = load(transaction, id)?;
        revision(transaction, &after, "consolidation.stale", context)?;
        let mut audit_after = audited(transaction, id)?;
        audit_after["reason"] = json!("stale");
        audit(
            transaction,
            context,
            "memory.archive",
            id,
            before.ok(),
            Some(audit_after),
        )?;
        outcome.archived.push(id.clone());
    }
    if !outcome.archived.is_empty() {
        bump_channel(transaction, &write.channel_id)?;
        journal::emit(transaction, context, &outcome.archived)?;
    }
    Ok(outcome)
}

/// `mark_verified`: `verified_at` e auditoria, limpa `stale_candidate`; sem revisão, sem
/// `version` e sem evento (009 Q-001).
pub(super) fn mark_verified(
    transaction: &Transaction,
    target: &MemoryTarget,
    context: &WriteContext,
) -> Result<Memory, StoreError> {
    let before = load_target(transaction, target)?;
    transaction
        .execute(
            "UPDATE memories SET verified_at = ?1, stale_candidate = 0 WHERE id = ?2",
            params![context.at, before.id],
        )
        .map_err(sql_error)?;
    let after = load(transaction, &before.id)?;
    audit(
        transaction,
        context,
        "memory.verify",
        &after.id,
        Some(json!({ "verified_at": before.verified_at })),
        Some(json!({ "verified_at": after.verified_at })),
    )?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}
