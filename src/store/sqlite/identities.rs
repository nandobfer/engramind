//! Apps, agentes e tokens no SQLite.

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, types::Value as SqlValue};

use super::{
    journal::{self, Entry, to_value},
    sql_error, unique_conflict,
};
use crate::{
    core::{
        ids,
        model::{
            Agent, AgentLookup, AgentPatch, App, AppPatch, AuthTokenRow, IdentityCounts, Lookup,
            NewAgent, NewApp, NewToken, Token, TokenFilter, TokenPatch, WriteContext, token_status,
        },
        pagination::{Cursor, Page, PageRequest},
        principal::{AgentRef, AppRef, ScopeSet, TokenKind},
    },
    store::StoreError,
};

const ID_ATTEMPTS: usize = 16;

const APP_COLUMNS: &str = "a.id, a.slug, a.name, a.description, a.created_at, a.disabled_at";
const AGENT_COLUMNS: &str =
    "g.id, g.slug, g.name, g.description, g.created_at, g.disabled_at, a.id, a.slug";
const TOKEN_COLUMNS: &str = "t.id, t.kind, t.scopes, t.label, t.expires_at, t.created_at, \
     t.last_used_at, t.revoked_at, a.id, a.slug, g.id, g.slug";
const TOKEN_FROM: &str =
    "FROM tokens t JOIN apps a ON a.id = t.app_id LEFT JOIN agents g ON g.id = t.agent_id";

fn app_from_row(row: &Row<'_>) -> rusqlite::Result<App> {
    Ok(App {
        id: row.get(0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        created_at: row.get(4)?,
        disabled_at: row.get(5)?,
    })
}

fn agent_from_row(row: &Row<'_>) -> rusqlite::Result<Agent> {
    let slug: String = row.get(1)?;
    let app_slug: String = row.get(7)?;
    Ok(Agent {
        id: row.get(0)?,
        app: AppRef {
            id: row.get(6)?,
            slug: app_slug.clone(),
        },
        qualified_slug: format!("{app_slug}/{slug}"),
        slug,
        name: row.get(2)?,
        description: row.get(3)?,
        created_at: row.get(4)?,
        disabled_at: row.get(5)?,
    })
}

fn token_from_row(row: &Row<'_>, now: &str) -> rusqlite::Result<Token> {
    let id: String = row.get(0)?;
    let kind: String = row.get(1)?;
    let scopes: String = row.get(2)?;
    let expires_at: Option<String> = row.get(4)?;
    let revoked_at: Option<String> = row.get(7)?;
    let app_slug: String = row.get(9)?;
    let agent_id: Option<String> = row.get(10)?;
    let agent_slug: Option<String> = row.get(11)?;
    let invalid = |column: usize, what: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            format!("{what} invalido").into(),
        )
    };
    Ok(Token {
        prefix: format!("eng.{id}"),
        status: token_status(revoked_at.as_deref(), expires_at.as_deref(), now),
        id,
        kind: TokenKind::parse(&kind).ok_or_else(|| invalid(1, "kind"))?,
        scopes: ScopeSet::parse(&scopes)
            .map_err(|_| invalid(2, "scopes"))?
            .scopes(),
        app: AppRef {
            id: row.get(8)?,
            slug: app_slug.clone(),
        },
        agent: agent_id.zip(agent_slug).map(|(id, slug)| AgentRef {
            id,
            slug: format!("{app_slug}/{slug}"),
        }),
        label: row.get(3)?,
        expires_at,
        created_at: row.get(5)?,
        last_used_at: row.get(6)?,
        revoked_at,
    })
}

/// Gera um id livre na tabela, com retentativa em colisão.
pub(super) fn free_id(
    transaction: &Transaction,
    table: &str,
    prefix: &str,
) -> Result<String, StoreError> {
    for _ in 0..ID_ATTEMPTS {
        let id = ids::generate(prefix);
        let taken: bool = transaction
            .query_row(
                &format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE id = ?1)"),
                params![id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if !taken {
            return Ok(id);
        }
    }
    Err(StoreError::Internal(format!("sem id livre em {table}")))
}

/// Consulta de uma listagem: `SELECT`, filtros e as colunas `(created_at, id)` da ordem.
struct Listing<'a> {
    select: &'a str,
    conditions: Vec<String>,
    values: Vec<SqlValue>,
    order: (&'a str, &'a str),
}

/// Página genérica ordenada por `(created_at, id)`.
fn page<T>(
    connection: &Connection,
    listing: Listing<'_>,
    page: &PageRequest,
    map: impl Fn(&Row<'_>) -> rusqlite::Result<T>,
    key: impl Fn(&T) -> Cursor,
) -> rusqlite::Result<Page<T>> {
    let Listing {
        select,
        mut conditions,
        mut values,
        order,
    } = listing;
    if let Some(after) = &page.after {
        conditions.push(format!("({}, {}) > (?, ?)", order.0, order.1));
        values.push(SqlValue::Text(after.key.clone()));
        values.push(SqlValue::Text(after.id.clone()));
    }
    let filter = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "{select}{filter} ORDER BY {}, {} LIMIT {}",
        order.0,
        order.1,
        page.limit + 1
    );
    let mut statement = connection.prepare(&sql)?;
    let mut items = statement
        .query_map(rusqlite::params_from_iter(values), |row| map(row))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let next = if items.len() > page.limit as usize {
        items.truncate(page.limit as usize);
        items.last().map(&key)
    } else {
        None
    };
    Ok(Page { items, next })
}

// ---------- leitura ----------

pub(super) fn find_auth_token(
    connection: &Connection,
    id: &str,
) -> rusqlite::Result<Option<AuthTokenRow>> {
    let now = ids::timestamp(ids::now());
    connection
        .query_row(
            &format!(
                "SELECT {TOKEN_COLUMNS}, t.secret_hash, a.disabled_at IS NOT NULL,
                        g.disabled_at IS NOT NULL
                 {TOKEN_FROM} WHERE t.id = ?1"
            ),
            params![id],
            |row| {
                Ok(AuthTokenRow {
                    token: token_from_row(row, &now)?,
                    digest: row.get(12)?,
                    app_disabled: row.get(13)?,
                    agent_disabled: row.get(14)?,
                })
            },
        )
        .optional()
}

pub(super) fn count_tokens(connection: &Connection) -> rusqlite::Result<u64> {
    connection.query_row("SELECT count(*) FROM tokens", [], |row| {
        row.get::<_, i64>(0).map(|count| count as u64)
    })
}

pub(super) fn identity_counts(connection: &Connection) -> rusqlite::Result<IdentityCounts> {
    let now = ids::timestamp(ids::now());
    connection.query_row(
        "SELECT (SELECT count(*) FROM apps), (SELECT count(*) FROM agents),
                (SELECT count(*) FROM tokens),
                (SELECT count(*) FROM tokens WHERE revoked_at IS NULL
                     AND (expires_at IS NULL OR expires_at > ?1))",
        params![now],
        |row| {
            Ok(IdentityCounts {
                apps: row.get::<_, i64>(0)? as u64,
                agents: row.get::<_, i64>(1)? as u64,
                tokens: row.get::<_, i64>(2)? as u64,
                active_tokens: row.get::<_, i64>(3)? as u64,
            })
        },
    )
}

pub(super) fn list_apps(
    connection: &Connection,
    request: &PageRequest,
) -> rusqlite::Result<Page<App>> {
    page(
        connection,
        Listing {
            select: &format!("SELECT {APP_COLUMNS} FROM apps a"),
            conditions: Vec::new(),
            values: Vec::new(),
            order: ("a.created_at", "a.id"),
        },
        request,
        app_from_row,
        |app| Cursor {
            key: app.created_at.clone(),
            id: app.id.clone(),
        },
    )
}

pub(super) fn get_app(connection: &Connection, lookup: &Lookup) -> rusqlite::Result<Option<App>> {
    let (column, value) = match lookup {
        Lookup::Id(id) => ("a.id", id),
        Lookup::Slug(slug) => ("a.slug", slug),
    };
    connection
        .query_row(
            &format!("SELECT {APP_COLUMNS} FROM apps a WHERE {column} = ?1"),
            params![value],
            app_from_row,
        )
        .optional()
}

pub(super) fn list_agents(
    connection: &Connection,
    app_id: Option<&str>,
    request: &PageRequest,
) -> rusqlite::Result<Page<Agent>> {
    let (conditions, values) = match app_id {
        Some(app_id) => (
            vec!["g.app_id = ?".to_owned()],
            vec![SqlValue::Text(app_id.to_owned())],
        ),
        None => (Vec::new(), Vec::new()),
    };
    page(
        connection,
        Listing {
            select: &format!("SELECT {AGENT_COLUMNS} FROM agents g JOIN apps a ON a.id = g.app_id"),
            conditions,
            values,
            order: ("g.created_at", "g.id"),
        },
        request,
        agent_from_row,
        |agent| Cursor {
            key: agent.created_at.clone(),
            id: agent.id.clone(),
        },
    )
}

pub(super) fn get_agent(
    connection: &Connection,
    lookup: &AgentLookup,
) -> rusqlite::Result<Option<Agent>> {
    let select = format!("SELECT {AGENT_COLUMNS} FROM agents g JOIN apps a ON a.id = g.app_id");
    match lookup {
        AgentLookup::Id(id) => connection
            .query_row(
                &format!("{select} WHERE g.id = ?1"),
                params![id],
                agent_from_row,
            )
            .optional(),
        AgentLookup::Slug { app, agent } => connection
            .query_row(
                &format!("{select} WHERE a.slug = ?1 AND g.slug = ?2"),
                params![app, agent],
                agent_from_row,
            )
            .optional(),
    }
}

pub(super) fn list_tokens(
    connection: &Connection,
    filter: &TokenFilter,
    request: &PageRequest,
) -> rusqlite::Result<Page<Token>> {
    let now = ids::timestamp(ids::now());
    let mut conditions = Vec::new();
    let mut values = Vec::new();
    if let Some(app_id) = &filter.app_id {
        conditions.push("t.app_id = ?".to_owned());
        values.push(SqlValue::Text(app_id.clone()));
    }
    if let Some(agent_id) = &filter.agent_id {
        conditions.push("t.agent_id = ?".to_owned());
        values.push(SqlValue::Text(agent_id.clone()));
    }
    page(
        connection,
        Listing {
            select: &format!("SELECT {TOKEN_COLUMNS} {TOKEN_FROM}"),
            conditions,
            values,
            order: ("t.created_at", "t.id"),
        },
        request,
        |row| token_from_row(row, &now),
        |token| Cursor {
            key: token.created_at.clone(),
            id: token.id.clone(),
        },
    )
}

pub(super) fn get_token(connection: &Connection, id: &str) -> rusqlite::Result<Option<Token>> {
    let now = ids::timestamp(ids::now());
    connection
        .query_row(
            &format!("SELECT {TOKEN_COLUMNS} {TOKEN_FROM} WHERE t.id = ?1"),
            params![id],
            |row| token_from_row(row, &now),
        )
        .optional()
}

// ---------- escrita ----------

/// Tokens `admin` ativos com `write`, de app e agente não desativados.
fn active_admin_writers(transaction: &Transaction, now: &str) -> Result<u64, StoreError> {
    transaction
        .query_row(
            "SELECT count(*) FROM tokens t JOIN apps a ON a.id = t.app_id
                 LEFT JOIN agents g ON g.id = t.agent_id
             WHERE t.kind = 'admin' AND (',' || t.scopes || ',') LIKE '%,write,%'
               AND t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at > ?1)
               AND a.disabled_at IS NULL AND (t.agent_id IS NULL OR g.disabled_at IS NULL)",
            params![now],
            |row| row.get::<_, i64>(0).map(|count| count as u64),
        )
        .map_err(sql_error)
}

/// Executa `change` e recusa com `LAST_ADMIN_TOKEN` se ela zerar os admins com `write` (FR-019).
fn guard_last_admin<R>(
    transaction: &Transaction,
    now: &str,
    change: impl FnOnce() -> Result<R, StoreError>,
) -> Result<R, StoreError> {
    let before = active_admin_writers(transaction, now)?;
    let result = change()?;
    if before > 0 && active_admin_writers(transaction, now)? == 0 {
        return Err(StoreError::Conflict("LAST_ADMIN_TOKEN"));
    }
    Ok(result)
}

fn load_app(transaction: &Transaction, id: &str) -> Result<App, StoreError> {
    get_app(transaction, &Lookup::Id(id.to_owned()))
        .map_err(sql_error)?
        .ok_or(StoreError::NotFound)
}

fn load_agent(transaction: &Transaction, id: &str) -> Result<Agent, StoreError> {
    get_agent(transaction, &AgentLookup::Id(id.to_owned()))
        .map_err(sql_error)?
        .ok_or(StoreError::NotFound)
}

fn load_token(transaction: &Transaction, id: &str) -> Result<Token, StoreError> {
    get_token(transaction, id)
        .map_err(sql_error)?
        .ok_or(StoreError::NotFound)
}

fn insert_app(transaction: &Transaction, new: &NewApp, at: &str) -> Result<App, StoreError> {
    let id = free_id(transaction, "apps", ids::APP_PREFIX)?;
    transaction
        .execute(
            "INSERT INTO apps (id, slug, name, description, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, new.slug, new.name, new.description, at],
        )
        .map_err(|error| unique_conflict(error, "SLUG_ALREADY_EXISTS"))?;
    load_app(transaction, &id)
}

pub(super) fn create_app(
    transaction: &Transaction,
    new: &NewApp,
    context: &WriteContext,
) -> Result<App, StoreError> {
    let app = insert_app(transaction, new, &context.at)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "app.create",
            entity: "app",
            entity_id: Some(&app.id),
            before: None,
            after: Some(to_value(&app)?),
        },
    )?;
    journal::remember(transaction, context, &app)?;
    Ok(app)
}

/// Aplica nome, descrição e desativação a `apps` ou `agents`.
fn apply_patch(
    transaction: &Transaction,
    table: &str,
    id: &str,
    patch: &AppPatch,
    at: &str,
) -> Result<(), StoreError> {
    if let Some(name) = &patch.name {
        transaction
            .execute(
                &format!("UPDATE {table} SET name = ?1 WHERE id = ?2"),
                params![name, id],
            )
            .map_err(sql_error)?;
    }
    if let Some(description) = &patch.description {
        transaction
            .execute(
                &format!("UPDATE {table} SET description = ?1 WHERE id = ?2"),
                params![description, id],
            )
            .map_err(sql_error)?;
    }
    match patch.disabled {
        Some(true) => {
            transaction
                .execute(
                    &format!(
                        "UPDATE {table} SET disabled_at = ?1 WHERE id = ?2 AND disabled_at IS NULL"
                    ),
                    params![at, id],
                )
                .map_err(sql_error)?;
        }
        Some(false) => {
            transaction
                .execute(
                    &format!("UPDATE {table} SET disabled_at = NULL WHERE id = ?1"),
                    params![id],
                )
                .map_err(sql_error)?;
        }
        None => {}
    }
    Ok(())
}

pub(super) fn update_app(
    transaction: &Transaction,
    id: &str,
    patch: &AppPatch,
    context: &WriteContext,
) -> Result<App, StoreError> {
    let before = load_app(transaction, id)?;
    let after = guard_last_admin(transaction, &context.at, || {
        apply_patch(transaction, "apps", id, patch, &context.at)?;
        load_app(transaction, id)
    })?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "app.update",
            entity: "app",
            entity_id: Some(id),
            before: Some(to_value(&before)?),
            after: Some(to_value(&after)?),
        },
    )?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

pub(super) fn delete_app(
    transaction: &Transaction,
    id: &str,
    context: &WriteContext,
) -> Result<App, StoreError> {
    let before = load_app(transaction, id)?;
    let mut audited = to_value(&before)?;
    audited["subscriptions_removed"] = serde_json::json!(super::channels::count_for_agents(
        transaction,
        "app_id",
        id
    )?);
    guard_last_admin(transaction, &context.at, || {
        transaction
            .execute("DELETE FROM apps WHERE id = ?1", params![id])
            .map_err(sql_error)
    })?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "app.delete",
            entity: "app",
            entity_id: Some(id),
            before: Some(audited),
            after: None,
        },
    )?;
    journal::remember(transaction, context, &before)?;
    Ok(before)
}

pub(super) fn create_agent(
    transaction: &Transaction,
    new: &NewAgent,
    context: &WriteContext,
) -> Result<Agent, StoreError> {
    let id = free_id(transaction, "agents", ids::AGENT_PREFIX)?;
    transaction
        .execute(
            "INSERT INTO agents (id, app_id, slug, name, description, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                new.app_id,
                new.slug,
                new.name,
                new.description,
                context.at
            ],
        )
        .map_err(|error| unique_conflict(error, "SLUG_ALREADY_EXISTS"))?;
    let agent = load_agent(transaction, &id)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "agent.create",
            entity: "agent",
            entity_id: Some(&id),
            before: None,
            after: Some(to_value(&agent)?),
        },
    )?;
    journal::remember(transaction, context, &agent)?;
    Ok(agent)
}

pub(super) fn update_agent(
    transaction: &Transaction,
    id: &str,
    patch: &AgentPatch,
    context: &WriteContext,
) -> Result<Agent, StoreError> {
    let before = load_agent(transaction, id)?;
    let after = guard_last_admin(transaction, &context.at, || {
        apply_patch(transaction, "agents", id, patch, &context.at)?;
        load_agent(transaction, id)
    })?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "agent.update",
            entity: "agent",
            entity_id: Some(id),
            before: Some(to_value(&before)?),
            after: Some(to_value(&after)?),
        },
    )?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

pub(super) fn delete_agent(
    transaction: &Transaction,
    id: &str,
    context: &WriteContext,
) -> Result<Agent, StoreError> {
    let before = load_agent(transaction, id)?;
    let mut audited = to_value(&before)?;
    audited["subscriptions_removed"] =
        serde_json::json!(super::channels::count_for_agents(transaction, "id", id)?);
    guard_last_admin(transaction, &context.at, || {
        transaction
            .execute("DELETE FROM agents WHERE id = ?1", params![id])
            .map_err(sql_error)
    })?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "agent.delete",
            entity: "agent",
            entity_id: Some(id),
            before: Some(audited),
            after: None,
        },
    )?;
    journal::remember(transaction, context, &before)?;
    Ok(before)
}

fn insert_token(transaction: &Transaction, new: &NewToken, at: &str) -> Result<Token, StoreError> {
    let id = match &new.id {
        Some(id) => id.clone(),
        None => free_id(transaction, "tokens", ids::TOKEN_PREFIX)?,
    };
    let scopes = ScopeSet::from_scopes(&new.scopes)
        .map_err(|error| StoreError::Internal(error.to_string()))?
        .to_csv();
    transaction
        .execute(
            "INSERT INTO tokens
                 (id, app_id, agent_id, kind, scopes, label, secret_hash, expires_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                new.app_id,
                new.agent_id,
                new.kind.as_str(),
                scopes,
                new.label,
                new.digest,
                new.expires_at,
                at
            ],
        )
        .map_err(sql_error)?;
    load_token(transaction, &id)
}

pub(super) fn create_token(
    transaction: &Transaction,
    new: &NewToken,
    context: &WriteContext,
) -> Result<Token, StoreError> {
    let token = insert_token(transaction, new, &context.at)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "token.issue",
            entity: "token",
            entity_id: Some(&token.id),
            before: None,
            after: Some(to_value(&token)?),
        },
    )?;
    journal::remember(transaction, context, &token)?;
    Ok(token)
}

pub(super) fn update_token(
    transaction: &Transaction,
    id: &str,
    patch: &TokenPatch,
    context: &WriteContext,
) -> Result<Token, StoreError> {
    let before = load_token(transaction, id)?;
    let after = guard_last_admin(transaction, &context.at, || {
        if let Some(label) = &patch.label {
            transaction
                .execute(
                    "UPDATE tokens SET label = ?1 WHERE id = ?2",
                    params![label, id],
                )
                .map_err(sql_error)?;
        }
        if let Some(scopes) = &patch.scopes {
            let scopes = ScopeSet::from_scopes(scopes)
                .map_err(|error| StoreError::Internal(error.to_string()))?
                .to_csv();
            transaction
                .execute(
                    "UPDATE tokens SET scopes = ?1 WHERE id = ?2",
                    params![scopes, id],
                )
                .map_err(sql_error)?;
        }
        if let Some(expires_at) = &patch.expires_at {
            transaction
                .execute(
                    "UPDATE tokens SET expires_at = ?1 WHERE id = ?2",
                    params![expires_at, id],
                )
                .map_err(sql_error)?;
        }
        load_token(transaction, id)
    })?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "token.update",
            entity: "token",
            entity_id: Some(id),
            before: Some(to_value(&before)?),
            after: Some(to_value(&after)?),
        },
    )?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

pub(super) fn revoke_token(
    transaction: &Transaction,
    id: &str,
    context: &WriteContext,
) -> Result<Token, StoreError> {
    let before = load_token(transaction, id)?;
    let after = guard_last_admin(transaction, &context.at, || {
        transaction
            .execute(
                "UPDATE tokens SET revoked_at = ?1 WHERE id = ?2 AND revoked_at IS NULL",
                params![context.at, id],
            )
            .map_err(sql_error)?;
        load_token(transaction, id)
    })?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "token.revoke",
            entity: "token",
            entity_id: Some(id),
            before: Some(to_value(&before)?),
            after: Some(to_value(&after)?),
        },
    )?;
    journal::remember(transaction, context, &after)?;
    Ok(after)
}

pub(super) fn bootstrap(
    transaction: &Transaction,
    app: &NewApp,
    token: &NewToken,
    context: &WriteContext,
) -> Result<Option<Token>, StoreError> {
    let existing: i64 = transaction
        .query_row("SELECT count(*) FROM tokens", [], |row| row.get(0))
        .map_err(sql_error)?;
    if existing > 0 {
        return Ok(None);
    }
    let app = match get_app(transaction, &Lookup::Slug(app.slug.clone())).map_err(sql_error)? {
        Some(app) => app,
        None => {
            let app = insert_app(transaction, app, &context.at)?;
            journal::audit(
                transaction,
                context,
                Entry {
                    action: "app.create",
                    entity: "app",
                    entity_id: Some(&app.id),
                    before: None,
                    after: Some(to_value(&app)?),
                },
            )?;
            app
        }
    };
    let new = NewToken {
        app_id: app.id,
        ..token.clone()
    };
    let token = insert_token(transaction, &new, &context.at)?;
    journal::audit(
        transaction,
        context,
        Entry {
            action: "token.bootstrap",
            entity: "token",
            entity_id: Some(&token.id),
            before: None,
            after: Some(to_value(&token)?),
        },
    )?;
    Ok(Some(token))
}

pub(super) fn touch_tokens(
    transaction: &Transaction,
    batch: &[(String, String)],
) -> rusqlite::Result<()> {
    let mut statement = transaction.prepare(
        "UPDATE tokens SET last_used_at = ?2
         WHERE id = ?1 AND (last_used_at IS NULL OR last_used_at < ?2)",
    )?;
    for (id, at) in batch {
        statement.execute(params![id, at])?;
    }
    Ok(())
}
