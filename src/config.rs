use std::{env, fmt, net::IpAddr, path::PathBuf, str::FromStr, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

/// Chave AES-256 (32 bytes) que cifra os segredos de configuração.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretKey([u8; 32]);

impl SecretKey {
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Development,
    Test,
    Staging,
    Production,
}

impl Environment {
    fn requires_explicit_hosts(self) -> bool {
        matches!(self, Self::Staging | Self::Production)
    }
}

impl FromStr for Environment {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "development" => Ok(Self::Development),
            "test" => Ok(Self::Test),
            "staging" => Ok(Self::Staging),
            "production" => Ok(Self::Production),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Staging => "staging",
            Self::Production => "production",
        })
    }
}

/// Hosts aceitos no cabeçalho `Host` do `/mcp` (proteção contra DNS rebinding).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllowedHosts {
    Any,
    List(Vec<String>),
}

impl Default for AllowedHosts {
    fn default() -> Self {
        Self::List(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])
    }
}

/// Configuração mínima para os comandos que só abrem o banco (`migrate`, `doctor`).
#[derive(Clone, Debug)]
pub struct DatabaseSettings {
    pub path: PathBuf,
    pub read_pool_size: usize,
    pub litestream_enabled: bool,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub environment: Environment,
    pub host: IpAddr,
    pub port: u16,
    pub database: DatabaseSettings,
    pub models_path: PathBuf,
    pub bootstrap_admin_token: Option<Secret>,
    pub token_pepper: Secret,
    pub secrets_key: SecretKey,
    pub request_timeout: Duration,
    pub max_body_bytes: usize,
    pub mcp_allowed_hosts: AllowedHosts,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("variavel obrigatoria ausente ou vazia: {0}")]
    Missing(&'static str),
    #[error("valor invalido para {name}: {value}")]
    Invalid { name: &'static str, value: String },
}

/// Leitura das variáveis, injetável nos testes (edition 2024 torna `env::set_var` inseguro).
pub trait Lookup {
    fn get(&self, name: &str) -> Option<String>;
}

struct ProcessEnv;

impl Lookup for ProcessEnv {
    fn get(&self, name: &str) -> Option<String> {
        env::var(name).ok()
    }
}

impl<F> Lookup for F
where
    F: Fn(&str) -> Option<String>,
{
    fn get(&self, name: &str) -> Option<String> {
        self(name)
    }
}

impl DatabaseSettings {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(&ProcessEnv)
    }

    pub fn from_lookup(lookup: &impl Lookup) -> Result<Self, ConfigError> {
        let read_pool_size = parse_or(lookup, "READ_POOL_SIZE", "4")?;
        if read_pool_size == 0 {
            return Err(ConfigError::Invalid {
                name: "READ_POOL_SIZE",
                value: "deve ser maior que zero".to_owned(),
            });
        }
        Ok(Self {
            path: PathBuf::from(
                optional(lookup, "DATABASE_PATH")
                    .unwrap_or_else(|| "./data/engramind.db".to_owned()),
            ),
            read_pool_size,
            litestream_enabled: parse_bool(lookup, "LITESTREAM_ENABLED", false)?,
        })
    }
}

impl Settings {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(&ProcessEnv)
    }

    pub fn from_lookup(lookup: &impl Lookup) -> Result<Self, ConfigError> {
        let environment_value =
            optional(lookup, "APP_ENV").unwrap_or_else(|| "development".to_owned());
        let environment =
            environment_value
                .parse::<Environment>()
                .map_err(|()| ConfigError::Invalid {
                    name: "APP_ENV",
                    value: "use development, test, staging ou production".to_owned(),
                })?;
        let host = parse_or(lookup, "APP_HOST", "0.0.0.0")?;
        let port = required(lookup, "APP_PORT")?;
        let port = port.parse::<u16>().map_err(|_| ConfigError::Invalid {
            name: "APP_PORT",
            value: port,
        })?;
        let database = DatabaseSettings::from_lookup(lookup)?;
        let models_path = PathBuf::from(
            optional(lookup, "MODELS_PATH").unwrap_or_else(|| "./data/models".to_owned()),
        );
        let bootstrap_admin_token = optional(lookup, "BOOTSTRAP_ADMIN_TOKEN").map(Secret::new);
        let token_pepper = Secret::new(required(lookup, "TOKEN_PEPPER")?);
        let secrets_key = parse_secrets_key(&required(lookup, "SECRETS_KEY")?)?;
        let request_timeout_secs = parse_or::<u64>(lookup, "REQUEST_TIMEOUT_SECS", "30")?;
        let max_body_bytes = parse_or(lookup, "MAX_BODY_BYTES", "1048576")?;
        let mcp_allowed_hosts = parse_mcp_allowed_hosts(
            environment,
            optional(lookup, "MCP_ALLOWED_HOSTS").as_deref(),
        )?;

        if token_pepper.expose().len() < 32 {
            return Err(ConfigError::Invalid {
                name: "TOKEN_PEPPER",
                value: "deve possuir pelo menos 32 caracteres".to_owned(),
            });
        }
        if request_timeout_secs == 0 {
            return Err(ConfigError::Invalid {
                name: "REQUEST_TIMEOUT_SECS",
                value: "deve ser maior que zero".to_owned(),
            });
        }
        if max_body_bytes == 0 {
            return Err(ConfigError::Invalid {
                name: "MAX_BODY_BYTES",
                value: "deve ser maior que zero".to_owned(),
            });
        }

        Ok(Self {
            environment,
            host,
            port,
            database,
            models_path,
            bootstrap_admin_token,
            token_pepper,
            secrets_key,
            request_timeout: Duration::from_secs(request_timeout_secs),
            max_body_bytes,
            mcp_allowed_hosts,
        })
    }

    pub fn address(&self) -> (IpAddr, u16) {
        (self.host, self.port)
    }
}

/// `SECRETS_KEY` precisa ser base64 de exatamente 32 bytes. O valor nunca aparece no erro.
fn parse_secrets_key(value: &str) -> Result<SecretKey, ConfigError> {
    let invalid = || ConfigError::Invalid {
        name: "SECRETS_KEY",
        value: "deve ser base64 de 32 bytes (openssl rand -base64 32)".to_owned(),
    };
    let mut bytes = BASE64.decode(value).map_err(|_| invalid())?;
    let key = <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| invalid());
    bytes.zeroize();
    Ok(SecretKey(key?))
}

/// Parses `MCP_ALLOWED_HOSTS`: a comma-separated list of hosts or `host:port`
/// authorities, or `*` to accept any Host. Staging and production must set it
/// explicitly because the MCP is reached through the public hostname.
fn parse_mcp_allowed_hosts(
    environment: Environment,
    value: Option<&str>,
) -> Result<AllowedHosts, ConfigError> {
    let invalid = |value: &str| ConfigError::Invalid {
        name: "MCP_ALLOWED_HOSTS",
        value: format!("{value}: use '*' ou hosts separados por virgula"),
    };
    let Some(value) = value else {
        return if environment.requires_explicit_hosts() {
            Err(ConfigError::Missing("MCP_ALLOWED_HOSTS"))
        } else {
            Ok(AllowedHosts::default())
        };
    };
    if value == "*" {
        return Ok(AllowedHosts::Any);
    }
    let hosts: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    if hosts.is_empty()
        || hosts.iter().any(|host| {
            host == "*"
                || host.contains('/')
                || axum::http::uri::Authority::try_from(host.as_str()).is_err()
        })
    {
        return Err(invalid(value));
    }
    Ok(AllowedHosts::List(hosts))
}

fn required(lookup: &impl Lookup, name: &'static str) -> Result<String, ConfigError> {
    optional(lookup, name).ok_or(ConfigError::Missing(name))
}

fn optional(lookup: &impl Lookup, name: &'static str) -> Option<String> {
    lookup
        .get(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_or<T>(lookup: &impl Lookup, name: &'static str, default: &str) -> Result<T, ConfigError>
where
    T: FromStr,
{
    let value = optional(lookup, name).unwrap_or_else(|| default.to_owned());
    value
        .parse()
        .map_err(|_| ConfigError::Invalid { name, value })
}

fn parse_bool(
    lookup: &impl Lookup,
    name: &'static str,
    default: bool,
) -> Result<bool, ConfigError> {
    match optional(lookup, name).as_deref() {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(value) => Err(ConfigError::Invalid {
            name,
            value: value.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const PEPPER: &str = "pepper-de-teste-com-mais-de-32-caracteres";
    const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn valid() -> Vec<(&'static str, &'static str)> {
        vec![
            ("APP_PORT", "8000"),
            ("TOKEN_PEPPER", PEPPER),
            ("SECRETS_KEY", KEY),
        ]
    }

    fn with(overrides: &[(&'static str, &'static str)]) -> Result<Settings, ConfigError> {
        let mut pairs = valid();
        pairs.retain(|(name, _)| !overrides.iter().any(|(other, _)| other == name));
        pairs.extend(overrides.iter().filter(|(_, value)| !value.is_empty()));
        Settings::from_lookup(&lookup(&pairs))
    }

    #[test]
    fn loads_defaults_with_only_required_variables() {
        let settings = with(&[]).expect("valid settings");
        assert_eq!(settings.environment, Environment::Development);
        assert_eq!(settings.port, 8000);
        assert_eq!(settings.database.path, PathBuf::from("./data/engramind.db"));
        assert_eq!(settings.database.read_pool_size, 4);
        assert!(!settings.database.litestream_enabled);
        assert_eq!(settings.request_timeout, Duration::from_secs(30));
        assert_eq!(settings.max_body_bytes, 1_048_576);
        assert_eq!(settings.mcp_allowed_hosts, AllowedHosts::default());
        assert_eq!(settings.secrets_key.expose()[31], 31);
    }

    #[test]
    fn required_variables_fail_startup_when_missing() {
        for name in ["APP_PORT", "TOKEN_PEPPER", "SECRETS_KEY"] {
            assert_eq!(
                with(&[(name, "")]).map(|_| ()),
                Err(ConfigError::Missing(name))
            );
        }
    }

    #[test]
    fn invalid_values_are_rejected_without_echoing_secrets() {
        let short = with(&[("TOKEN_PEPPER", "curto-demais")]).expect_err("short pepper");
        assert!(!short.to_string().contains("curto-demais"));

        for key in ["nao-e-base64!", "AAECAwQ="] {
            let error = with(&[("SECRETS_KEY", key)]).expect_err("invalid key");
            assert!(matches!(
                error,
                ConfigError::Invalid {
                    name: "SECRETS_KEY",
                    ..
                }
            ));
            assert!(!error.to_string().contains(key));
        }

        for (name, value) in [
            ("APP_PORT", "porta"),
            ("APP_ENV", "prod"),
            ("READ_POOL_SIZE", "0"),
            ("REQUEST_TIMEOUT_SECS", "0"),
            ("MAX_BODY_BYTES", "0"),
            ("LITESTREAM_ENABLED", "yes"),
        ] {
            assert!(with(&[(name, value)]).is_err(), "{name}={value} accepted");
        }
    }

    #[test]
    fn production_requires_explicit_mcp_hosts() {
        assert_eq!(
            with(&[("APP_ENV", "production")]).map(|_| ()),
            Err(ConfigError::Missing("MCP_ALLOWED_HOSTS"))
        );
        let settings = with(&[
            ("APP_ENV", "production"),
            ("MCP_ALLOWED_HOSTS", " Engramind-API:8000 , localhost "),
        ])
        .expect("explicit hosts");
        assert_eq!(
            settings.mcp_allowed_hosts,
            AllowedHosts::List(vec![
                "engramind-api:8000".to_owned(),
                "localhost".to_owned()
            ])
        );
        assert_eq!(
            parse_mcp_allowed_hosts(Environment::Staging, Some("*")),
            Ok(AllowedHosts::Any)
        );
        for invalid in [",", "a,*", "http://host", "host/path", "bad host"] {
            assert!(
                parse_mcp_allowed_hosts(Environment::Production, Some(invalid)).is_err(),
                "{invalid} accepted"
            );
        }
    }

    #[test]
    fn secret_debug_output_is_redacted() {
        let settings = with(&[]).expect("valid settings");
        let output = format!("{settings:?}");
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains(PEPPER));
        assert_eq!(format!("{:?}", settings.secrets_key), "[REDACTED]");
    }
}
