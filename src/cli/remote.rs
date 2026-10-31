//! Comandos clientes: cada um vira a chamada REST da operação correspondente do catálogo.

use std::process::ExitCode;

use anyhow::bail;
use reqwest::Method;
use serde_json::{Map, Value, json};
use zeroize::Zeroize as _;

use super::{
    AgentsCommand, AppsCommand, AuditCommand, ChannelSettingsCommand, ChannelsCommand, Command,
    ConnectionArgs, DocsCommand, EmbeddingsCommand, JobsCommand, LlmCallsCommand, MemoriesCommand,
    ProfilesCommand, SecretsCommand, SettingsCommand, SubscriptionsCommand, TokensCommand,
    UpdateArgs,
    client::{self, ApiRequest, Connection, Outcome, segment},
    output,
    profiles::{self, Profile},
};

/// Resolve a conexão: flags e variáveis de ambiente, depois o perfil escolhido.
fn connection(args: &ConnectionArgs) -> Result<Connection, String> {
    let config = profiles::load().map_err(|error| format!("{error:#}"))?;
    let name = args.profile.clone().or(config.default_profile.clone());
    let profile = match &name {
        Some(name) => match config.profiles.get(name) {
            Some(profile) => Some(profile.clone()),
            None if args.profile.is_some() => return Err(format!("perfil inexistente: {name}")),
            None => None,
        },
        None => None,
    };
    let url = args
        .url
        .clone()
        .or_else(|| profile.as_ref().map(|profile| profile.url.clone()))
        .filter(|url| !url.trim().is_empty())
        .ok_or("defina --url, ENGRAMIND_URL ou um perfil (engramind profiles add)")?;
    let token = args
        .token
        .clone()
        .or_else(|| profile.as_ref().map(|profile| profile.token.clone()))
        .filter(|token| !token.trim().is_empty())
        .ok_or("defina --token, ENGRAMIND_TOKEN ou um perfil (engramind profiles add)")?;
    let channels = args
        .channels
        .clone()
        .or_else(|| profile.and_then(|profile| profile.channels));
    Ok(Connection {
        url,
        token,
        channels,
    })
}

fn scopes(value: &str) -> Value {
    Value::Array(
        value
            .split(',')
            .map(str::trim)
            .filter(|scope| !scope.is_empty())
            .map(|scope| Value::String(scope.to_owned()))
            .collect(),
    )
}

/// Texto opcional da CLI: `""` vira `null` (limpa o campo).
fn nullable(value: String) -> Value {
    if value.is_empty() {
        Value::Null
    } else {
        json!(value)
    }
}

fn update_body(changes: UpdateArgs) -> Value {
    let mut body = Map::new();
    if let Some(name) = changes.name {
        body.insert("name".to_owned(), json!(name));
    }
    if let Some(description) = changes.description {
        body.insert(
            "description".to_owned(),
            if description.is_empty() {
                Value::Null
            } else {
                json!(description)
            },
        );
    }
    if changes.disable {
        body.insert("disabled".to_owned(), json!(true));
    } else if changes.enable {
        body.insert("disabled".to_owned(), json!(false));
    }
    Value::Object(body)
}

/// Lê o valor de `settings set` como JSON; texto solto vira string.
fn setting_value(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_owned()))
}

fn memory_path(memory: &str, suffix: &str) -> String {
    format!("/v1/memories/{}{suffix}", segment(memory))
}

/// Lista separada por vírgula como array JSON.
fn list(value: Option<String>) -> Option<Value> {
    value.map(|value| {
        Value::Array(
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| Value::String(item.to_owned()))
                .collect(),
        )
    })
}

fn memories_request(command: MemoriesCommand) -> ApiRequest {
    match command {
        MemoriesCommand::List {
            categories,
            status,
            author,
            since,
            fields,
            page,
        } => ApiRequest::new(Method::GET, "/v1/memories".to_owned())
            .query("categories", categories)
            .query("status", status)
            .query("author", author)
            .query("since", since)
            .query("fields", fields.fields)
            .query("cursor", page.cursor)
            .query("limit", page.limit),
        MemoriesCommand::Get { memory } => ApiRequest::new(Method::GET, memory_path(&memory, "")),
        MemoriesCommand::Search {
            query,
            categories,
            limit,
            include_archived,
            exclude_ids,
            exclude_context,
            fields,
        } => {
            let mut body = Map::new();
            body.insert("query".to_owned(), json!(query.join(" ")));
            if let Some(categories) = list(categories) {
                body.insert("categories".to_owned(), categories);
            }
            if let Some(limit) = limit {
                body.insert("limit".to_owned(), json!(limit));
            }
            if include_archived {
                body.insert("include_archived".to_owned(), json!(true));
            }
            if let Some(exclude_ids) = list(exclude_ids) {
                body.insert("exclude_ids".to_owned(), exclude_ids);
            }
            if exclude_context {
                body.insert("exclude_context".to_owned(), json!(true));
            }
            if let Some(fields) = fields.fields {
                body.insert("fields".to_owned(), json!(fields));
            }
            ApiRequest::new(Method::POST, "/v1/memories/search".to_owned())
                .body(Value::Object(body))
        }
        MemoriesCommand::History { memory, page } => {
            ApiRequest::new(Method::GET, memory_path(&memory, "/history"))
                .query("cursor", page.cursor)
                .query("limit", page.limit)
        }
        MemoriesCommand::Create {
            channel,
            text,
            category,
            status,
            origin_source,
            origin_label,
            idempotency,
        } => {
            let mut body = Map::new();
            body.insert("channel".to_owned(), json!(channel));
            body.insert("text".to_owned(), json!(text));
            if let Some(category) = category {
                body.insert("category".to_owned(), json!(category));
            }
            if let Some(status) = status {
                body.insert("status".to_owned(), json!(status));
            }
            if let Some(source) = origin_source {
                body.insert(
                    "origin".to_owned(),
                    json!({ "source": source, "label": origin_label }),
                );
            }
            ApiRequest::new(Method::POST, "/v1/memories".to_owned())
                .body(Value::Object(body))
                .idempotency(idempotency.idempotency_key)
        }
        MemoriesCommand::Update {
            memory,
            text,
            category,
            expected_version,
            idempotency,
        } => {
            let mut body = Map::new();
            if let Some(text) = text {
                body.insert("text".to_owned(), json!(text));
            }
            if let Some(category) = category {
                body.insert("category".to_owned(), json!(category));
            }
            if let Some(version) = expected_version {
                body.insert("expected_version".to_owned(), json!(version));
            }
            ApiRequest::new(Method::PATCH, memory_path(&memory, ""))
                .body(Value::Object(body))
                .idempotency(idempotency.idempotency_key)
        }
        MemoriesCommand::Archive {
            memory,
            idempotency,
        } => ApiRequest::new(Method::POST, memory_path(&memory, "/archive"))
            .idempotency(idempotency.idempotency_key),
        MemoriesCommand::Restore {
            memory,
            idempotency,
        } => ApiRequest::new(Method::POST, memory_path(&memory, "/restore"))
            .idempotency(idempotency.idempotency_key),
        MemoriesCommand::Pin {
            memory,
            idempotency,
        } => ApiRequest::new(Method::POST, memory_path(&memory, "/pin"))
            .idempotency(idempotency.idempotency_key),
        MemoriesCommand::Unpin {
            memory,
            idempotency,
        } => ApiRequest::new(Method::POST, memory_path(&memory, "/unpin"))
            .idempotency(idempotency.idempotency_key),
        MemoriesCommand::Move {
            memory,
            to,
            idempotency,
        } => ApiRequest::new(Method::POST, memory_path(&memory, "/move"))
            .body(json!({ "to": to }))
            .idempotency(idempotency.idempotency_key),
        MemoriesCommand::Revert {
            memory,
            to_version,
            idempotency,
        } => ApiRequest::new(Method::POST, memory_path(&memory, "/revert"))
            .body(json!({ "version": to_version }))
            .idempotency(idempotency.idempotency_key),
        MemoriesCommand::Delete {
            memory,
            idempotency,
        } => ApiRequest::new(Method::DELETE, memory_path(&memory, ""))
            .idempotency(idempotency.idempotency_key),
    }
}

/// `--text -` lê o texto da entrada padrão (multilinha, sem escapes).
fn read_stdin_text(command: Command) -> Result<Command, String> {
    fn read(text: &mut String) -> Result<(), String> {
        if text != "-" {
            return Ok(());
        }
        let mut buffer = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)
            .map_err(|error| format!("falha ao ler a entrada padrao: {error}"))?;
        *text = buffer;
        Ok(())
    }
    let mut command = command;
    match &mut command {
        Command::Memories(MemoriesCommand::Create { text, .. }) => read(text)?,
        Command::Memories(MemoriesCommand::Update {
            text: Some(text), ..
        }) => read(text)?,
        Command::Settings(SettingsCommand::TestRedator { text, .. }) => read(text)?,
        Command::Settings(SettingsCommand::TestEmbedding { text, .. }) => read(text)?,
        Command::Settings(SettingsCommand::EmbeddingModels {
            api_key_stdin: true,
            api_key,
            ..
        }) => {
            let mut buffer = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)
                .map_err(|error| format!("falha ao ler a entrada padrao: {error}"))?;
            let secret = buffer.trim().to_owned();
            buffer.zeroize();
            if !secret.is_empty() {
                *api_key = Some(secret);
            }
        }
        Command::Secrets(SecretsCommand::Set {
            delete: false,
            value,
            ..
        }) => {
            let mut buffer = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)
                .map_err(|error| format!("falha ao ler a entrada padrao: {error}"))?;
            let secret = buffer.trim().to_owned();
            buffer.zeroize();
            if secret.is_empty() {
                return Err(
                    "informe o segredo pela entrada padrao (ou use --delete para apagar)"
                        .to_owned(),
                );
            }
            *value = Some(secret);
        }
        _ => {}
    }
    Ok(command)
}

/// A chamada REST do comando, pelo mesmo caminho do catálogo de operações.
pub fn request(command: Command) -> ApiRequest {
    match command {
        Command::Whoami => ApiRequest::new(Method::GET, "/v1/whoami".to_owned()),
        Command::Context {
            format,
            if_none_match,
        } => ApiRequest::new(Method::GET, "/v1/context".to_owned())
            .query("format", format)
            .if_none_match(if_none_match),
        Command::Apps(command) => match command {
            AppsCommand::List { page } => ApiRequest::new(Method::GET, "/v1/apps".to_owned())
                .query("cursor", page.cursor)
                .query("limit", page.limit),
            AppsCommand::Get { app } => {
                ApiRequest::new(Method::GET, format!("/v1/apps/{}", segment(&app)))
            }
            AppsCommand::Create {
                slug,
                name,
                description,
                idempotency,
            } => ApiRequest::new(Method::POST, "/v1/apps".to_owned())
                .body(json!({ "slug": slug, "name": name, "description": description }))
                .idempotency(idempotency.idempotency_key),
            AppsCommand::Update {
                app,
                changes,
                idempotency,
            } => ApiRequest::new(Method::PATCH, format!("/v1/apps/{}", segment(&app)))
                .body(update_body(changes))
                .idempotency(idempotency.idempotency_key),
            AppsCommand::Delete { app, idempotency } => {
                ApiRequest::new(Method::DELETE, format!("/v1/apps/{}", segment(&app)))
                    .idempotency(idempotency.idempotency_key)
            }
        },
        Command::Agents(command) => match command {
            AgentsCommand::List { app, page } => {
                ApiRequest::new(Method::GET, "/v1/agents".to_owned())
                    .query("app", app)
                    .query("cursor", page.cursor)
                    .query("limit", page.limit)
            }
            AgentsCommand::Get { agent } => {
                ApiRequest::new(Method::GET, format!("/v1/agents/{}", segment(&agent)))
            }
            AgentsCommand::Create {
                app,
                slug,
                name,
                description,
                idempotency,
            } => ApiRequest::new(Method::POST, "/v1/agents".to_owned())
                .body(json!({ "app": app, "slug": slug, "name": name, "description": description }))
                .idempotency(idempotency.idempotency_key),
            AgentsCommand::Update {
                agent,
                changes,
                idempotency,
            } => ApiRequest::new(Method::PATCH, format!("/v1/agents/{}", segment(&agent)))
                .body(update_body(changes))
                .idempotency(idempotency.idempotency_key),
            AgentsCommand::Delete { agent, idempotency } => {
                ApiRequest::new(Method::DELETE, format!("/v1/agents/{}", segment(&agent)))
                    .idempotency(idempotency.idempotency_key)
            }
        },
        Command::Tokens(command) => match command {
            TokensCommand::List { app, agent, page } => {
                ApiRequest::new(Method::GET, "/v1/tokens".to_owned())
                    .query("app", app)
                    .query("agent", agent)
                    .query("cursor", page.cursor)
                    .query("limit", page.limit)
            }
            TokensCommand::Issue {
                app,
                agent,
                kind,
                scopes: values,
                label,
                expires_at,
                idempotency,
            } => ApiRequest::new(Method::POST, "/v1/tokens".to_owned())
                .body(json!({
                    "app": app,
                    "agent": agent,
                    "kind": kind,
                    "scopes": scopes(&values),
                    "label": label,
                    "expires_at": expires_at,
                }))
                .idempotency(idempotency.idempotency_key),
            TokensCommand::Update {
                token_id: token,
                label,
                scopes: values,
                expires_at,
                no_expiration,
                idempotency,
            } => {
                let mut body = Map::new();
                if let Some(label) = label {
                    body.insert(
                        "label".to_owned(),
                        if label.is_empty() {
                            Value::Null
                        } else {
                            json!(label)
                        },
                    );
                }
                if let Some(values) = values {
                    body.insert("scopes".to_owned(), scopes(&values));
                }
                if let Some(expires_at) = expires_at {
                    body.insert("expires_at".to_owned(), json!(expires_at));
                } else if no_expiration {
                    body.insert("expires_at".to_owned(), Value::Null);
                }
                ApiRequest::new(Method::PATCH, format!("/v1/tokens/{}", segment(&token)))
                    .body(Value::Object(body))
                    .idempotency(idempotency.idempotency_key)
            }
            TokensCommand::Revoke {
                token_id: token,
                idempotency,
            } => ApiRequest::new(
                Method::POST,
                format!("/v1/tokens/{}/revoke", segment(&token)),
            )
            .idempotency(idempotency.idempotency_key),
        },
        Command::Channels(command) => match command {
            ChannelsCommand::List {
                include_archived,
                page,
            } => ApiRequest::new(Method::GET, "/v1/channels".to_owned())
                .query("include_archived", include_archived.then_some(true))
                .query("cursor", page.cursor)
                .query("limit", page.limit),
            ChannelsCommand::Get { channel } => {
                ApiRequest::new(Method::GET, format!("/v1/channels/{}", segment(&channel)))
            }
            ChannelsCommand::Create {
                slug,
                name,
                description,
                priority,
                idempotency,
            } => ApiRequest::new(Method::POST, "/v1/channels".to_owned())
                .body(json!({
                    "slug": slug,
                    "name": name,
                    "description": description,
                    "priority": priority,
                }))
                .idempotency(idempotency.idempotency_key),
            ChannelsCommand::Update {
                channel,
                name,
                description,
                priority,
                idempotency,
            } => {
                let mut body = Map::new();
                if let Some(name) = name {
                    body.insert("name".to_owned(), json!(name));
                }
                if let Some(description) = description {
                    body.insert("description".to_owned(), nullable(description));
                }
                if let Some(priority) = priority {
                    body.insert("priority".to_owned(), json!(priority));
                }
                ApiRequest::new(Method::PATCH, format!("/v1/channels/{}", segment(&channel)))
                    .body(Value::Object(body))
                    .idempotency(idempotency.idempotency_key)
            }
            ChannelsCommand::Archive {
                channel,
                idempotency,
            } => ApiRequest::new(
                Method::POST,
                format!("/v1/channels/{}/archive", segment(&channel)),
            )
            .idempotency(idempotency.idempotency_key),
            ChannelsCommand::Restore {
                channel,
                idempotency,
            } => ApiRequest::new(
                Method::POST,
                format!("/v1/channels/{}/restore", segment(&channel)),
            )
            .idempotency(idempotency.idempotency_key),
            ChannelsCommand::Delete {
                channel,
                idempotency,
            } => ApiRequest::new(
                Method::DELETE,
                format!("/v1/channels/{}", segment(&channel)),
            )
            .idempotency(idempotency.idempotency_key),
            ChannelsCommand::Stats { channel } => ApiRequest::new(
                Method::GET,
                format!("/v1/channels/{}/stats", segment(&channel)),
            ),
            ChannelsCommand::Settings(command) => match command {
                ChannelSettingsCommand::Get { channel } => ApiRequest::new(
                    Method::GET,
                    format!("/v1/channels/{}/settings", segment(&channel)),
                ),
                ChannelSettingsCommand::Set {
                    channel,
                    key,
                    value,
                    idempotency,
                } => {
                    let mut body = Map::new();
                    body.insert(key, setting_value(&value));
                    ApiRequest::new(
                        Method::PUT,
                        format!("/v1/channels/{}/settings", segment(&channel)),
                    )
                    .body(Value::Object(body))
                    .idempotency(idempotency.idempotency_key)
                }
            },
        },
        Command::Memories(command) => memories_request(command),
        Command::Subscriptions(command) => match command {
            SubscriptionsCommand::List {
                agent,
                channel,
                page,
            } => ApiRequest::new(Method::GET, "/v1/subscriptions".to_owned())
                .query("agent", agent)
                .query("channel", channel)
                .query("cursor", page.cursor)
                .query("limit", page.limit),
            SubscriptionsCommand::Add {
                agent,
                target,
                mode,
                access,
                priority,
                idempotency,
            } => ApiRequest::new(Method::POST, "/v1/subscriptions".to_owned())
                .body(json!({
                    "agent": agent,
                    "target": target,
                    "mode": mode,
                    "access": access.as_deref().map(scopes),
                    "priority": priority,
                }))
                .idempotency(idempotency.idempotency_key),
            SubscriptionsCommand::Update {
                subscription,
                mode,
                access,
                token_access,
                priority,
                channel_priority,
                idempotency,
            } => {
                let mut body = Map::new();
                if let Some(mode) = mode {
                    body.insert("mode".to_owned(), json!(mode));
                }
                if let Some(access) = access {
                    body.insert("access".to_owned(), scopes(&access));
                } else if token_access {
                    body.insert("access".to_owned(), Value::Null);
                }
                if let Some(priority) = priority {
                    body.insert("priority".to_owned(), json!(priority));
                } else if channel_priority {
                    body.insert("priority".to_owned(), Value::Null);
                }
                ApiRequest::new(
                    Method::PATCH,
                    format!("/v1/subscriptions/{}", segment(&subscription)),
                )
                .body(Value::Object(body))
                .idempotency(idempotency.idempotency_key)
            }
            SubscriptionsCommand::Remove {
                subscription,
                idempotency,
            } => ApiRequest::new(
                Method::DELETE,
                format!("/v1/subscriptions/{}", segment(&subscription)),
            )
            .idempotency(idempotency.idempotency_key),
        },
        Command::Settings(command) => match command {
            SettingsCommand::Get { key } => {
                ApiRequest::new(Method::GET, "/v1/settings".to_owned()).query("key", key)
            }
            SettingsCommand::Set {
                key,
                value,
                idempotency,
            } => {
                let mut body = Map::new();
                body.insert(key, setting_value(&value));
                ApiRequest::new(Method::PUT, "/v1/settings".to_owned())
                    .body(Value::Object(body))
                    .idempotency(idempotency.idempotency_key)
            }
            SettingsCommand::TestRedator {
                text,
                settings,
                idempotency,
            } => {
                let mut body = Map::new();
                body.insert("text".to_owned(), json!(text));
                if let Some(settings) = settings {
                    body.insert("settings".to_owned(), setting_value(&settings));
                }
                ApiRequest::new(Method::POST, "/v1/settings/redator/test".to_owned())
                    .body(Value::Object(body))
                    .idempotency(idempotency.idempotency_key)
            }
            SettingsCommand::TestEmbedding {
                text,
                settings,
                idempotency,
            } => {
                let mut body = Map::new();
                body.insert("text".to_owned(), json!(text));
                if let Some(settings) = settings {
                    body.insert("settings".to_owned(), setting_value(&settings));
                }
                ApiRequest::new(Method::POST, "/v1/settings/embedding/test".to_owned())
                    .body(Value::Object(body))
                    .idempotency(idempotency.idempotency_key)
            }
            SettingsCommand::EmbeddingModels {
                provider,
                base_url,
                api_key,
                idempotency,
                ..
            } => {
                let mut body = Map::new();
                body.insert("provider".to_owned(), json!(provider));
                if let Some(base_url) = base_url {
                    body.insert("base_url".to_owned(), json!(base_url));
                }
                if let Some(api_key) = api_key {
                    body.insert("api_key".to_owned(), json!(api_key));
                }
                ApiRequest::new(Method::POST, "/v1/settings/embedding/models".to_owned())
                    .body(Value::Object(body))
                    .idempotency(idempotency.idempotency_key)
            }
        },
        Command::Hints { text, limit } => {
            let mut body = Map::new();
            body.insert("text".to_owned(), json!(text.join(" ")));
            if let Some(limit) = limit {
                body.insert("limit".to_owned(), json!(limit));
            }
            ApiRequest::new(Method::POST, "/v1/hints".to_owned()).body(Value::Object(body))
        }
        Command::Embeddings(EmbeddingsCommand::Reembed { idempotency }) => {
            ApiRequest::new(Method::POST, "/v1/embeddings/reembed".to_owned())
                .idempotency(idempotency.idempotency_key)
        }
        Command::Secrets(SecretsCommand::Set {
            key,
            value,
            idempotency,
            ..
        }) => ApiRequest::new(Method::PUT, format!("/v1/secrets/{}", segment(&key)))
            .body(json!({ "value": value }))
            .idempotency(idempotency.idempotency_key),
        Command::Status => ApiRequest::new(Method::GET, "/v1/status".to_owned()),
        Command::Jobs(JobsCommand::List { state, kind, page }) => {
            ApiRequest::new(Method::GET, "/v1/jobs".to_owned())
                .query("state", state)
                .query("kind", kind)
                .query("cursor", page.cursor)
                .query("limit", page.limit)
        }
        Command::Jobs(JobsCommand::Retry { job, idempotency }) => {
            ApiRequest::new(Method::POST, format!("/v1/jobs/{}/retry", segment(&job)))
                .idempotency(idempotency.idempotency_key)
        }
        Command::LlmCalls(LlmCallsCommand::List {
            since,
            purpose,
            page,
        }) => ApiRequest::new(Method::GET, "/v1/llm-calls".to_owned())
            .query("since", since)
            .query("purpose", purpose)
            .query("cursor", page.cursor)
            .query("limit", page.limit),
        Command::Audit(AuditCommand::List {
            entity,
            entity_id,
            action,
            page,
        }) => ApiRequest::new(Method::GET, "/v1/audit".to_owned())
            .query("entity", entity)
            .query("entity_id", entity_id)
            .query("action", action)
            .query("cursor", page.cursor)
            .query("limit", page.limit),
        Command::Docs(command) => match command {
            DocsCommand::Overview => ApiRequest::new(Method::GET, "/v1/docs/overview".to_owned()),
            DocsCommand::Guides => ApiRequest::new(Method::GET, "/v1/docs/guides".to_owned()),
            DocsCommand::Guide { guide, section } => {
                ApiRequest::new(Method::GET, format!("/v1/docs/guides/{}", segment(&guide)))
                    .query("section", section)
            }
            DocsCommand::Search { terms, limit } => {
                ApiRequest::new(Method::GET, "/v1/docs/search".to_owned())
                    .query("q", Some(terms.join(" ")))
                    .query("limit", limit)
            }
            DocsCommand::Operations { guide, tag } => {
                ApiRequest::new(Method::GET, "/v1/docs/operations".to_owned())
                    .query("guide", guide)
                    .query("tag", tag)
            }
            DocsCommand::Operation { operation } => ApiRequest::new(
                Method::GET,
                format!("/v1/docs/operations/{}", segment(&operation)),
            ),
            DocsCommand::Schema { schema } => ApiRequest::new(
                Method::GET,
                format!("/v1/docs/schemas/{}", segment(&schema)),
            ),
        },
        Command::Serve
        | Command::Migrate
        | Command::Doctor
        | Command::Help
        | Command::Profiles(_) => unreachable!("comandos locais nao chamam a API"),
    }
}

pub async fn run(command: Command, args: &ConnectionArgs, as_json: bool) -> ExitCode {
    let issuing = matches!(command, Command::Tokens(TokensCommand::Issue { .. }));
    let context = matches!(command, Command::Context { .. });
    let connection = match connection(args) {
        Ok(connection) => connection,
        Err(message) => {
            output::error_envelope(&client::local_error("CLI_CONFIG_INVALID", message), as_json);
            return ExitCode::from(client::local_exit_code("CLI_CONFIG_INVALID"));
        }
    };
    let command = match read_stdin_text(command) {
        Ok(command) => command,
        Err(message) => {
            output::error_envelope(&client::local_error("CLI_INPUT_INVALID", message), as_json);
            return ExitCode::from(client::local_exit_code("CLI_INPUT_INVALID"));
        }
    };
    match client::send(&connection, request(command)).await {
        Outcome::Success(body) => {
            if as_json {
                output::json(&body);
            } else if context {
                output::context(&body);
            } else {
                output::human(&body);
                if issuing && body["secret"].is_string() {
                    output::warn("guarde o segredo agora: ele nao sera mostrado de novo");
                }
            }
            ExitCode::SUCCESS
        }
        Outcome::Failure { envelope, exit } => {
            output::error_envelope(&envelope, as_json);
            ExitCode::from(exit)
        }
    }
}

pub fn profiles(
    command: ProfilesCommand,
    connection: &ConnectionArgs,
    as_json: bool,
) -> anyhow::Result<ExitCode> {
    let mut config = profiles::load()?;
    match command {
        ProfilesCommand::Add { name, default } => {
            profiles::validate_name(&name)?;
            let Some(url) = connection.url.clone().filter(|url| !url.trim().is_empty()) else {
                bail!("profiles add exige --url (ou ENGRAMIND_URL)");
            };
            let Some(token) = connection
                .token
                .clone()
                .filter(|token| !token.trim().is_empty())
            else {
                bail!("profiles add exige --token (ou ENGRAMIND_TOKEN)");
            };
            let channels = connection.channels.clone();
            config.profiles.insert(
                name.clone(),
                Profile {
                    url,
                    token,
                    channels,
                },
            );
            if default || config.default_profile.is_none() {
                config.default_profile = Some(name.clone());
            }
            let path = profiles::save(&config)?;
            report(
                as_json,
                json!({ "saved": name, "path": path.display().to_string() }),
            );
        }
        ProfilesCommand::List => {
            let list: Vec<Value> = config
                .profiles
                .iter()
                .map(|(name, profile)| {
                    json!({
                        "name": name,
                        "url": profile.url,
                        "token": profiles::mask(&profile.token),
                        "channels": profile.channels,
                        "default": config.default_profile.as_deref() == Some(name.as_str()),
                    })
                })
                .collect();
            report(as_json, json!({ "profiles": list }));
        }
        ProfilesCommand::Remove { name } => {
            if config.profiles.remove(&name).is_none() {
                bail!("perfil inexistente: {name}");
            }
            if config.default_profile.as_deref() == Some(name.as_str()) {
                config.default_profile = None;
            }
            profiles::save(&config)?;
            report(as_json, json!({ "removed": name }));
        }
        ProfilesCommand::Use { name } => {
            if !config.profiles.contains_key(&name) {
                bail!("perfil inexistente: {name}");
            }
            config.default_profile = Some(name.clone());
            profiles::save(&config)?;
            report(as_json, json!({ "default_profile": name }));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn report(as_json: bool, value: Value) {
    if as_json {
        output::json(&value);
    } else {
        output::human(&value);
    }
}
