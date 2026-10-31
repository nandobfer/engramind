//! Canais e assinaturas no SQLite.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use super::{
    identities::free_id,
    journal::{self, Entry, to_value},
    sql_error, unique_conflict,
};
use crate::{
    core::{
        access::subscription_pattern,
        ids,
        model::{
            Channel, ChannelPatch, Lookup, Mode, Network, NewChannel, NewSubscription,
            SettingChange, Subscription, SubscriptionPatch, WriteContext,
        },
        principal::{AgentRef, ScopeSet},
    },
    store::StoreError,
};

const CHANNEL_COLUMNS: &str =
    "c.id, c.slug, c.name, c.description, c.priority, c.version, c.created_at, c.archived_at";
const SUBSCRIPTION_SELECT: &str = "SELECT s.id, s.target, s.is_prefix, s.mode, s.access, \
     s.priority, s.created_at, g.id, a.slug, g.slug \
     FROM subscriptions s JOIN agents g ON g.id = s.agent_id JOIN apps a ON a.id = g.app_id";

fn channel_from_row(row: &Row<'_>) -> rusqlite::Result<Channel> {
    Ok(Channel {
        id: row.get(0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        priority: row.get(4)?,
        version: row.get(5)?,
        created_at: row.get(6)?,
        archived_at: row.get(7)?,
        reach: None,
    })
}

fn invalid_column(index: usize, message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, message.into())
}

fn subscription_from_row(row: &Row<'_>) -> rusqlite::Result<Subscription> {
    let target: String = row.get(1)?;
    let is_prefix: bool = row.get(2)?;
    let mode: String = row.get(3)?;
    let access: Option<String> = row.get(4)?;
    let app_slug: String = row.get(8)?;
    let agent_slug: String = row.get(9)?;
    Ok(Subscription {
        id: row.get(0)?,
        agent: AgentRef {
            id: row.get(7)?,
            slug: format!("{app_slug}/{agent_slug}"),
        },
        target: if is_prefix {
            format!("{target}*")
        } else {
            target
        },
        is_prefix,
        mode: Mode::parse(&mode).ok_or_else(|| invalid_column(3, format!("modo: {mode}")))?,
        access: access
            .map(|csv| {
                ScopeSet::parse(&csv)
                    .map(ScopeSet::scopes)
                    .map_err(|error| invalid_column(4, error.to_string()))
            })
            .transpose()?,
        priority: row.get(5)?,
        created_at: row.get(6)?,
        reaches: Vec::new(),
    })
}

/// Canais ativos que a assinatura alcança agora.
fn with_reaches(mut subscription: Subscription, channels: &[Channel]) -> Subscription {
    let pattern = subscription_pattern(&subscription);
    subscription.reaches = channels
        .iter()
        .filter(|channel| channel.archived_at.is_none() && pattern.matches(&channel.slug))
        .map(|channel| channel.slug.clone())
        .collect();
    subscription
}

fn all_channels(connection: &Connection) -> rusqlite::Result<Vec<Channel>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {CHANNEL_COLUMNS} FROM channels c ORDER BY c.slug"
    ))?;
    statement
        .query_map([], channel_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
}

// ---------- leitura ----------

pub(super) fn network(connection: &Connection) -> rusqlite::Result<Network> {
    let channels = all_channels(connection)?;
    let mut statement = connection.prepare(&format!(
        "{SUBSCRIPTION_SELECT} ORDER BY s.created_at, s.id"
    ))?;
    let subscriptions = statement
        .query_map([], subscription_from_row)?
        .map(|row| row.map(|subscription| with_reaches(subscription, &channels)))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut statement =
        connection.prepare("SELECT id, settings_json FROM channels WHERE settings_json <> '{}'")?;
    let channel_settings = statement
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let json: String = row.get(1)?;
            Ok((id, serde_json::from_str(&json).unwrap_or_default()))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Network {
        channels,
        subscriptions,
        channel_settings,
    })
}

pub(super) fn get_channel(
    connection: &Connection,
    lookup: &Lookup,
) -> rusqlite::Result<Option<Channel>> {
    let (column, value) = match lookup {
        Lookup::Id(id) => ("c.id", id),
        Lookup::Slug(slug) => ("c.slug", slug),
    };
    connection
        .query_row(
            &format!("SELECT {CHANNEL_COLUMNS} FROM channels c WHERE {column} = ?1"),
            params![value],
            channel_from_row,
        )
        .optional()
}

fn load_channel(transaction: &Transaction, id: &str) -> Result<Channel, StoreError> {
    get_channel(transaction, &Lookup::Id(id.to_owned()))
        .map_err(sql_error)?
        .ok_or(StoreError::NotFound)
}

fn load_subscription(transaction: &Transaction, id: &str) -> Result<Subscription, StoreError> {
    let subscription = transaction
        .query_row(
            &format!("{SUBSCRIPTION_SELECT} WHERE s.id = ?1"),
            params![id],
            subscription_from_row,
        )
        .optional()
        .map_err(sql_error)?
        .ok_or(StoreError::NotFound)?;
    let channels = all_channels(transaction).map_err(sql_error)?;
    Ok(with_reaches(subscription, &channels))
}

// ---------- canais ----------

pub(super) fn create_channel(
    transaction: &Transaction,
    new: &NewChannel,
    context: &WriteContext,
) -> Result<Channel, StoreError> {
    let id = free_id(transaction, "channels", ids::CHANNEL_PREFIX)?;
    transaction
        .execute(
            "INSERT INTO channels (id, slug, name, description, priority, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                new.slug,
                new.name,
                new.description,
                new.priority,
                context.at
            ],
        )
        .map_err(|error| unique_conflict(error, "SLUG_ALREADY_EXISTS"))?;
    let channel = load_channel(transaction, &id)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "channel.create",
            entity: "channel",
            entity_id: Some(&id),
            before: None,
            after: Some(to_value(&channel)?),
        },
    )?;
    journal::emit(transaction, context, &[])?;
    journal::remember(transaction, context, &channel)?;
    Ok(channel)
}

pub(super) fn update_channel(
    transaction: &Transaction,
    id: &str,
    patch: &ChannelPatch,
    context: &WriteContext,
) -> Result<Channel, StoreError> {
    let before = load_channel(transaction, id)?;
    let name = patch.name.as_ref().unwrap_or(&before.name);
    let description = patch.description.as_ref().unwrap_or(&before.description);
    let priority = patch.priority.unwrap_or(before.priority);
    transaction
        .execute(
            "UPDATE channels SET name = ?1, description = ?2, priority = ?3,
                 version = version + 1
             WHERE id = ?4",
            params![name, description, priority, id],
        )
        .map_err(sql_error)?;
    let after = load_channel(transaction, id)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "channel.update",
            entity: "channel",
            entity_id: Some(id),
            before: Some(to_value(&before)?),
            after: Some(to_value(&after)?),
        },
    )?;
    journal::emit(transaction, context, &[])?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

pub(super) fn set_channel_archived(
    transaction: &Transaction,
    id: &str,
    archived: bool,
    context: &WriteContext,
) -> Result<Channel, StoreError> {
    let before = load_channel(transaction, id)?;
    if before.archived_at.is_some() == archived {
        journal::remember(transaction, context, &before)?;
        return Ok(before);
    }
    let archived_at = archived.then_some(context.at.as_str());
    transaction
        .execute(
            "UPDATE channels SET archived_at = ?1, version = version + 1 WHERE id = ?2",
            params![archived_at, id],
        )
        .map_err(sql_error)?;
    let after = load_channel(transaction, id)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: if archived {
                "channel.archive"
            } else {
                "channel.restore"
            },
            entity: "channel",
            entity_id: Some(id),
            before: Some(to_value(&before)?),
            after: Some(to_value(&after)?),
        },
    )?;
    journal::emit(transaction, context, &[])?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

pub(super) fn delete_channel(
    transaction: &Transaction,
    id: &str,
    context: &WriteContext,
) -> Result<Channel, StoreError> {
    let before = load_channel(transaction, id)?;
    if before.archived_at.is_none() {
        return Err(StoreError::Conflict("CHANNEL_NOT_ARCHIVED"));
    }
    // Antes de apagar o canal, para o evento levar a última `channel_version`.
    journal::emit(transaction, context, &[])?;
    let removed = transaction
        .execute(
            "DELETE FROM subscriptions WHERE is_prefix = 0 AND target = ?1",
            params![before.slug],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "DELETE FROM memory_revisions WHERE memory_id IN
                 (SELECT id FROM memories WHERE channel_id = ?1)",
            params![id],
        )
        .map_err(sql_error)?;
    let memories = transaction
        .execute("DELETE FROM memories WHERE channel_id = ?1", params![id])
        .map_err(sql_error)?;
    transaction
        .execute("DELETE FROM channels WHERE id = ?1", params![id])
        .map_err(sql_error)?;
    let mut audited = to_value(&before)?;
    audited["subscriptions_removed"] = serde_json::json!(removed);
    audited["memories_removed"] = serde_json::json!(memories);
    journal::audit(
        transaction,
        context,
        Entry {
            action: "channel.delete",
            entity: "channel",
            entity_id: Some(id),
            before: Some(audited),
            after: None,
        },
    )?;
    journal::remember(transaction, context, &before)?;
    Ok(before)
}

/// Aplica as mudanças em `channels.settings_json` (`None` remove a sobreposição) e devolve as
/// sobreposições resultantes. Incrementa `channels.version`.
pub(super) fn update_channel_settings(
    transaction: &Transaction,
    id: &str,
    changes: &[SettingChange],
    context: &WriteContext,
) -> Result<BTreeMap<String, serde_json::Value>, StoreError> {
    load_channel(transaction, id)?;
    let json: String = transaction
        .query_row(
            "SELECT settings_json FROM channels WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    let before: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&json).unwrap_or_default();
    let mut after = before.clone();
    for change in changes {
        match &change.value_json {
            Some(value) => {
                let value = serde_json::from_str(value)
                    .map_err(|error| StoreError::Internal(error.to_string()))?;
                after.insert(change.key.clone(), value);
            }
            None => {
                after.remove(&change.key);
            }
        }
    }
    let json =
        serde_json::to_string(&after).map_err(|error| StoreError::Internal(error.to_string()))?;
    transaction
        .execute(
            "UPDATE channels SET settings_json = ?1, version = version + 1 WHERE id = ?2",
            params![json, id],
        )
        .map_err(sql_error)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "channel.settings_update",
            entity: "channel",
            entity_id: Some(id),
            before: Some(to_value(&before)?),
            after: Some(to_value(&after)?),
        },
    )?;
    journal::emit(transaction, context, &[])?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

// ---------- assinaturas ----------

fn access_csv(
    access: Option<&[crate::core::principal::Scope]>,
) -> Result<Option<String>, StoreError> {
    access
        .map(|scopes| {
            ScopeSet::from_scopes(scopes)
                .map(ScopeSet::to_csv)
                .map_err(|error| StoreError::Internal(error.to_string()))
        })
        .transpose()
}

pub(super) fn create_subscription(
    transaction: &Transaction,
    new: &NewSubscription,
    context: &WriteContext,
) -> Result<Subscription, StoreError> {
    if !new.is_prefix {
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM channels WHERE slug = ?1)",
                params![new.target],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if !exists {
            return Err(StoreError::Conflict("CHANNEL_NOT_FOUND"));
        }
    }
    let id = free_id(transaction, "subscriptions", ids::SUBSCRIPTION_PREFIX)?;
    transaction
        .execute(
            "INSERT INTO subscriptions
                 (id, agent_id, target, is_prefix, mode, access, priority, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                new.agent_id,
                new.target,
                new.is_prefix,
                new.mode.as_str(),
                access_csv(new.access.as_deref())?,
                new.priority,
                context.at
            ],
        )
        .map_err(|error| unique_conflict(error, "SUBSCRIPTION_EXISTS"))?;
    let subscription = load_subscription(transaction, &id)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "subscription.create",
            entity: "subscription",
            entity_id: Some(&id),
            before: None,
            after: Some(to_value(&subscription)?),
        },
    )?;
    journal::emit_with(
        transaction,
        context,
        &[],
        &[("subscription", serde_json::json!(id))],
    )?;
    journal::remember(transaction, context, &subscription)?;
    Ok(subscription)
}

pub(super) fn update_subscription(
    transaction: &Transaction,
    id: &str,
    patch: &SubscriptionPatch,
    context: &WriteContext,
) -> Result<Subscription, StoreError> {
    let before = load_subscription(transaction, id)?;
    let mode = patch.mode.unwrap_or(before.mode);
    let access = match &patch.access {
        Some(access) => access.clone(),
        None => before.access.clone(),
    };
    let priority = patch.priority.unwrap_or(before.priority);
    transaction
        .execute(
            "UPDATE subscriptions SET mode = ?1, access = ?2, priority = ?3 WHERE id = ?4",
            params![mode.as_str(), access_csv(access.as_deref())?, priority, id],
        )
        .map_err(sql_error)?;
    let after = load_subscription(transaction, id)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "subscription.update",
            entity: "subscription",
            entity_id: Some(id),
            before: Some(to_value(&before)?),
            after: Some(to_value(&after)?),
        },
    )?;
    journal::emit(transaction, context, &[])?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

pub(super) fn delete_subscription(
    transaction: &Transaction,
    id: &str,
    context: &WriteContext,
) -> Result<Subscription, StoreError> {
    let before = load_subscription(transaction, id)?;
    transaction
        .execute("DELETE FROM subscriptions WHERE id = ?1", params![id])
        .map_err(sql_error)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "subscription.delete",
            entity: "subscription",
            entity_id: Some(id),
            before: Some(to_value(&before)?),
            after: None,
        },
    )?;
    journal::emit(transaction, context, &[])?;
    journal::remember(transaction, context, &before)?;
    Ok(before)
}

/// Assinaturas que a exclusão de agentes vai remover em cascata.
pub(super) fn count_for_agents(
    transaction: &Transaction,
    filter: &str,
    id: &str,
) -> Result<i64, StoreError> {
    transaction
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM subscriptions WHERE agent_id IN \
                 (SELECT id FROM agents WHERE {filter} = ?1)"
            ),
            params![id],
            |row| row.get(0),
        )
        .map_err(sql_error)
}
