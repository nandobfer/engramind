//! Comandos clientes: cada um vira a chamada REST da operação correspondente do catálogo.

use std::process::ExitCode;

use anyhow::bail;
use reqwest::Method;
use serde_json::{Map, Value, json};

use super::{
    AgentsCommand, AppsCommand, AuditCommand, Command, ConnectionArgs, DocsCommand,
    ProfilesCommand, SettingsCommand, TokensCommand, UpdateArgs,
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

/// A chamada REST do comando, pelo mesmo caminho do catálogo de operações.
pub fn request(command: Command) -> ApiRequest {
    match command {
        Command::Whoami => ApiRequest::new(Method::GET, "/v1/whoami".to_owned()),
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
        },
        Command::Status => ApiRequest::new(Method::GET, "/v1/status".to_owned()),
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
    let connection = match connection(args) {
        Ok(connection) => connection,
        Err(message) => {
            output::error_envelope(&client::local_error("CLI_CONFIG_INVALID", message), as_json);
            return ExitCode::from(client::local_exit_code("CLI_CONFIG_INVALID"));
        }
    };
    match client::send(&connection, request(command)).await {
        Outcome::Success(body) => {
            if as_json {
                output::json(&body);
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
