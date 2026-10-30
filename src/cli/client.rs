//! Cliente HTTP da CLI: a CLI fala com a API como qualquer outro sistema (só `serve`,
//! `migrate` e `doctor` abrem o banco).

use std::time::Duration;

use reqwest::Method;
use serde_json::{Value, json};

use crate::error::exit_code_for;

const TIMEOUT: Duration = Duration::from_secs(30);

/// Erros locais da CLI, fora do catálogo do servidor (documentados no guia `cli`).
pub const LOCAL_ERRORS: &[(&str, u8, &str)] = &[
    (
        "CLI_CONFIG_INVALID",
        2,
        "URL ou token ausentes, perfil inexistente ou arquivo de perfis inválido.",
    ),
    (
        "CLI_CONNECTION_FAILED",
        8,
        "Servidor inacessível: rede, DNS, TLS ou tempo limite.",
    ),
    (
        "CLI_UNEXPECTED_RESPONSE",
        1,
        "Resposta fora do contrato (por exemplo, HTML de um proxy).",
    ),
];

pub fn local_exit_code(code: &str) -> u8 {
    LOCAL_ERRORS
        .iter()
        .find(|(name, _, _)| *name == code)
        .map_or(1, |(_, exit, _)| *exit)
}

pub fn local_error(code: &str, message: impl Into<String>) -> Value {
    json!({ "error": { "code": code, "message": message.into() } })
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub url: String,
    pub token: String,
    pub channels: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiRequest {
    pub method: Method,
    pub path: String,
    pub query: Vec<(&'static str, String)>,
    pub body: Option<Value>,
    pub idempotency_key: Option<String>,
}

impl ApiRequest {
    pub fn new(method: Method, path: String) -> Self {
        Self {
            method,
            path,
            query: Vec::new(),
            body: None,
            idempotency_key: None,
        }
    }

    pub fn query(mut self, name: &'static str, value: Option<impl ToString>) -> Self {
        if let Some(value) = value {
            self.query.push((name, value.to_string()));
        }
        self
    }

    pub fn body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    pub fn idempotency(mut self, key: Option<String>) -> Self {
        self.idempotency_key = key;
        self
    }
}

/// Resultado: corpo de sucesso, ou envelope de erro com o código de saída.
pub enum Outcome {
    Success(Value),
    Failure { envelope: Value, exit: u8 },
}

/// Codifica um segmento de caminho (`app-a/curador` → `app-a%2Fcurador`).
pub fn segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub async fn send(connection: &Connection, request: ApiRequest) -> Outcome {
    let client = match reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("engramind-cli/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return failure("CLI_CONNECTION_FAILED", error.to_string());
        }
    };
    let url = format!("{}{}", connection.url.trim_end_matches('/'), request.path);
    let mut builder = client
        .request(request.method, url)
        .bearer_auth(&connection.token)
        .query(&request.query);
    if let Some(channels) = &connection.channels {
        builder = builder.header("Engramind-Channels", channels);
    }
    if let Some(key) = &request.idempotency_key {
        builder = builder.header("Idempotency-Key", key);
    }
    if let Some(body) = &request.body {
        builder = builder.json(body);
    }
    let response = match builder.send().await {
        Ok(response) => response,
        Err(error) => {
            return failure(
                "CLI_CONNECTION_FAILED",
                format!(
                    "falha ao contatar {}: {}",
                    connection.url,
                    without_url(&error)
                ),
            );
        }
    };
    let status = response.status();
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => return failure("CLI_CONNECTION_FAILED", without_url(&error)),
    };
    let body: Option<Value> = serde_json::from_str(&text).ok();
    if status.is_success() {
        return Outcome::Success(body.unwrap_or(Value::Null));
    }
    match body.filter(|body| body["error"]["code"].is_string()) {
        Some(envelope) => {
            let exit = exit_code_for(envelope["error"]["code"].as_str().unwrap_or_default());
            Outcome::Failure { envelope, exit }
        }
        None => Outcome::Failure {
            envelope: local_error(
                "CLI_UNEXPECTED_RESPONSE",
                format!("resposta HTTP {status} fora do contrato"),
            ),
            exit: if status.is_server_error() { 8 } else { 1 },
        },
    }
}

fn failure(code: &str, message: String) -> Outcome {
    Outcome::Failure {
        envelope: local_error(code, message),
        exit: local_exit_code(code),
    }
}

/// Mensagem do `reqwest` sem a URL (que pode carregar query de terceiros).
fn without_url(error: &reqwest::Error) -> String {
    let kind = if error.is_timeout() {
        "tempo limite"
    } else if error.is_connect() {
        "conexao recusada ou inacessivel"
    } else if error.is_decode() {
        "resposta ilegivel"
    } else {
        "falha de transporte"
    };
    kind.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_encode_slashes_and_spaces() {
        assert_eq!(segment("app-a/curador"), "app-a%2Fcurador");
        assert_eq!(segment("tk_3f9e1a7c"), "tk_3f9e1a7c");
        assert_eq!(segment("a b"), "a%20b");
    }
}
