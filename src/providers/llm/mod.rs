//! Cliente LLM próprio sobre `reqwest` (desenho §3 e §12, decisão 0017), com saída JSON
//! estruturada nos três adaptadores. A resposta é sempre validada de novo por quem chama.

mod anthropic;
mod gemini;
mod openai_compatible;

use std::{future::Future, time::Duration};

use serde_json::Value;
use thiserror::Error;
use zeroize::Zeroizing;

pub use self::{anthropic::Anthropic, gemini::Gemini, openai_compatible::OpenAiCompatible};

/// Uma chamada: o prompt do sistema, a mensagem do usuário e o esquema da saída.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmRequest {
    pub model: String,
    pub system: String,
    pub user: String,
    /// Nome do esquema (`json_schema.name` e nome da tool forçada).
    pub schema_name: &'static str,
    /// JSON Schema do objeto esperado.
    pub schema: Value,
    pub max_output_tokens: u32,
    /// `None` omite o campo do corpo (padrão do provedor; os modelos que recusam
    /// `temperature` só funcionam assim).
    pub temperature: Option<f64>,
}

/// O objeto JSON devolvido e o uso informado pelo provedor.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    pub content: Value,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    /// Só quando o provedor informa (por exemplo `usage.cost` do OpenRouter).
    pub cost_usd: Option<f64>,
}

/// Falhas sem payload do provedor. `Rejected` leva só o status e a mensagem de erro do
/// provedor, higienizada e curta (`provider_reason`).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LlmError {
    #[error("tempo limite")]
    Timeout,
    #[error("limite do provedor (HTTP 429)")]
    RateLimited,
    #[error("provedor indisponivel")]
    Unavailable,
    #[error("recusado pelo provedor ({0})")]
    Rejected(String),
    #[error("resposta invalida")]
    InvalidResponse,
}

impl LlmError {
    /// `outcome` gravado em `llm_calls`.
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::Rejected(_) => "rejected",
            Self::InvalidResponse => "invalid_response",
        }
    }

    /// Indisponibilidade, limite e tempo limite passam para o próximo de `fallback_models`.
    pub fn tries_fallback(&self) -> bool {
        matches!(self, Self::Timeout | Self::RateLimited | Self::Unavailable)
    }
}

pub trait LlmClient: Send + Sync {
    fn complete_json(
        &self,
        request: LlmRequest,
    ) -> impl Future<Output = Result<LlmResponse, LlmError>> + Send;
}

/// Provedores aceitos em `redator.provider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAiCompatible,
    Anthropic,
    Gemini,
}

impl Provider {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai_compatible" => Some(Self::OpenAiCompatible),
            "anthropic" => Some(Self::Anthropic),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    /// URL oficial; o OpenAI-compatível não tem (a `base_url` é obrigatória).
    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::OpenAiCompatible => None,
            Self::Anthropic => Some("https://api.anthropic.com"),
            Self::Gemini => Some("https://generativelanguage.googleapis.com"),
        }
    }

    /// A chave é opcional só no OpenAI-compatível (servidores locais).
    pub fn requires_api_key(self) -> bool {
        !matches!(self, Self::OpenAiCompatible)
    }
}

/// Destino HTTP comum aos adaptadores.
#[derive(Clone)]
pub struct Endpoint {
    pub http: reqwest::Client,
    /// Sem `/` final.
    pub base_url: String,
    pub api_key: Option<Zeroizing<String>>,
    pub timeout: Duration,
}

impl Endpoint {
    pub fn new(
        http: reqwest::Client,
        base_url: &str,
        api_key: Option<Zeroizing<String>>,
        timeout: Duration,
    ) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key,
            timeout,
        }
    }
}

/// O adaptador do provedor.
#[derive(Clone)]
pub enum AnyClient {
    OpenAiCompatible(OpenAiCompatible),
    Anthropic(Anthropic),
    Gemini(Gemini),
}

impl AnyClient {
    pub fn new(provider: Provider, endpoint: Endpoint) -> Self {
        match provider {
            Provider::OpenAiCompatible => Self::OpenAiCompatible(OpenAiCompatible::new(endpoint)),
            Provider::Anthropic => Self::Anthropic(Anthropic::new(endpoint)),
            Provider::Gemini => Self::Gemini(Gemini::new(endpoint)),
        }
    }
}

impl LlmClient for AnyClient {
    async fn complete_json(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        match self {
            Self::OpenAiCompatible(client) => client.complete_json(request).await,
            Self::Anthropic(client) => client.complete_json(request).await,
            Self::Gemini(client) => client.complete_json(request).await,
        }
    }
}

/// Cliente HTTP compartilhado (rustls, sem redirecionamento automático).
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("engramind/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("cliente HTTP")
}

/// Resposta HTTP que não foi 2xx: status e um trecho do corpo, só para decidir o que fazer
/// (nunca registrado).
pub(crate) struct HttpFailure {
    pub(crate) status: u16,
    pub(crate) body: String,
    /// `provider_reason` do corpo, já sem a chave.
    pub(crate) reason: String,
}

pub(crate) enum PostError {
    Llm(LlmError),
    Http(HttpFailure),
}

impl From<PostError> for LlmError {
    fn from(error: PostError) -> Self {
        match error {
            PostError::Llm(error) => error,
            PostError::Http(failure) => classify_status(failure.status, &failure.reason),
        }
    }
}

pub(crate) fn classify_status(status: u16, reason: &str) -> LlmError {
    match status {
        429 => LlmError::RateLimited,
        408 => LlmError::Timeout,
        500..=599 => LlmError::Unavailable,
        other if reason.is_empty() => LlmError::Rejected(format!("HTTP {other}")),
        other => LlmError::Rejected(format!("HTTP {other}: {reason}")),
    }
}

/// Caracteres máximos do motivo do provedor (o `Rejected` inteiro fica abaixo de 200).
const MAX_REASON_CHARS: usize = 160;

/// O motivo de uma recusa, como o provedor o descreveu: `error.message` (com `error.type`
/// ou `error.status`) nos formatos OpenAI, Anthropic e Gemini, ou o corpo em texto. Uma
/// linha, sem controles, sem a chave da API (se ecoada) e com no máximo
/// `MAX_REASON_CHARS` caracteres. Corpo HTML ou vazio não tem motivo.
pub(crate) fn provider_reason(body: &str, api_key: Option<&str>) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let from_json = parsed.as_ref().and_then(|value| {
        let error = &value["error"];
        let message = error["message"]
            .as_str()
            .or_else(|| error.as_str())
            .or_else(|| value["message"].as_str())?;
        let kind = error["type"]
            .as_str()
            .or_else(|| error["status"].as_str())
            .or_else(|| value["type"].as_str().filter(|kind| *kind != "error"));
        Some(match kind {
            Some(kind) if !kind.is_empty() => format!("{message} ({kind})"),
            _ => message.to_owned(),
        })
    });
    let raw = match from_json {
        Some(text) => text,
        None if parsed.is_some() || body.trim_start().starts_with('<') => String::new(),
        None => body.to_owned(),
    };
    let mut text = raw
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(key) = api_key.map(str::trim).filter(|key| key.len() >= 8) {
        text = text.replace(key, "[REDACTED]");
    }
    if text.chars().count() > MAX_REASON_CHARS {
        text = text.chars().take(MAX_REASON_CHARS - 1).collect::<String>();
        text.push('…');
    }
    text
}

pub(crate) fn transport(error: &reqwest::Error) -> LlmError {
    if error.is_timeout() {
        LlmError::Timeout
    } else {
        LlmError::Unavailable
    }
}

fn failure(endpoint: &Endpoint, status: u16, text: &str) -> HttpFailure {
    HttpFailure {
        status,
        body: text.chars().take(2000).collect(),
        reason: provider_reason(text, endpoint.api_key.as_ref().map(|key| key.as_str())),
    }
}

/// `POST` JSON com o tempo limite do destino; devolve o corpo JSON de uma resposta 2xx.
pub(crate) async fn post_json(
    endpoint: &Endpoint,
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> Result<Value, PostError> {
    let mut request = endpoint.http.post(url).timeout(endpoint.timeout).json(body);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request
        .send()
        .await
        .map_err(|error| PostError::Llm(transport(&error)))?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|error| PostError::Llm(transport(&error)))?;
    if !(200..300).contains(&status) {
        return Err(PostError::Http(failure(endpoint, status, &text)));
    }
    serde_json::from_str(&text).map_err(|_| PostError::Llm(LlmError::InvalidResponse))
}

/// `GET` com o tempo limite do destino; devolve o corpo JSON de uma resposta 2xx.
pub(crate) async fn get_json(
    endpoint: &Endpoint,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Value, PostError> {
    let mut request = endpoint.http.get(url).timeout(endpoint.timeout);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request
        .send()
        .await
        .map_err(|error| PostError::Llm(transport(&error)))?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|error| PostError::Llm(transport(&error)))?;
    if !(200..300).contains(&status) {
        return Err(PostError::Http(failure(endpoint, status, &text)));
    }
    serde_json::from_str(&text).map_err(|_| PostError::Llm(LlmError::InvalidResponse))
}

/// O objeto JSON de um texto, aceitando uma cerca de código em volta.
fn parse_object(text: &str) -> Result<Value, LlmError> {
    let trimmed = text.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.trim_end().strip_suffix("```"))
        .unwrap_or(trimmed);
    match serde_json::from_str::<Value>(inner.trim()) {
        Ok(value @ Value::Object(_)) => Ok(value),
        _ => Err(LlmError::InvalidResponse),
    }
}

fn token_count(value: &Value) -> Option<i64> {
    value.as_i64().filter(|count| *count >= 0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn objects_are_parsed_with_or_without_a_code_fence() {
        assert_eq!(
            parse_object("{\"title\": \"t\"}").expect("plain"),
            json!({ "title": "t" })
        );
        assert_eq!(
            parse_object("```json\n{\"title\": \"t\"}\n```").expect("fence"),
            json!({ "title": "t" })
        );
        assert_eq!(parse_object("[1]"), Err(LlmError::InvalidResponse));
        assert_eq!(parse_object("titulo"), Err(LlmError::InvalidResponse));
    }

    #[test]
    fn statuses_map_to_retryable_or_rejected_errors() {
        assert_eq!(classify_status(429, ""), LlmError::RateLimited);
        assert_eq!(classify_status(503, "x"), LlmError::Unavailable);
        assert_eq!(classify_status(408, ""), LlmError::Timeout);
        assert_eq!(
            classify_status(401, ""),
            LlmError::Rejected("HTTP 401".to_owned())
        );
        assert_eq!(
            classify_status(400, "modelo inexistente"),
            LlmError::Rejected("HTTP 400: modelo inexistente".to_owned())
        );
        assert!(LlmError::Timeout.tries_fallback());
        assert!(!LlmError::InvalidResponse.tries_fallback());
        assert!(!LlmError::Rejected("HTTP 400".to_owned()).tries_fallback());
    }

    #[test]
    fn the_provider_reason_comes_from_each_error_format() {
        let anthropic = r#"{"type":"error","error":{"type":"invalid_request_error","message":"`temperature` is deprecated for this model."}}"#;
        assert_eq!(
            provider_reason(anthropic, None),
            "`temperature` is deprecated for this model. (invalid_request_error)"
        );
        let openai = r#"{"error":{"message":"The model `x` does not exist","type":"invalid_request_error","code":"model_not_found"}}"#;
        assert_eq!(
            provider_reason(openai, None),
            "The model `x` does not exist (invalid_request_error)"
        );
        let gemini =
            r#"{"error":{"code":400,"message":"API key not valid.","status":"INVALID_ARGUMENT"}}"#;
        assert_eq!(
            provider_reason(gemini, None),
            "API key not valid. (INVALID_ARGUMENT)"
        );
        assert_eq!(provider_reason("<html><body>Bad</body></html>", None), "");
        assert_eq!(provider_reason("", None), "");
        assert_eq!(provider_reason(r#"{"detail":1}"#, None), "");
        assert_eq!(
            provider_reason("linha 1\n\tlinha   2\r\n", None),
            "linha 1 linha 2"
        );
    }

    #[test]
    fn the_provider_reason_hides_the_key_and_is_short() {
        let key = "sk-test-0123456789abcdef";
        let body = format!(r#"{{"error":{{"message":"Incorrect API key provided: {key}."}}}}"#);
        let reason = provider_reason(&body, Some(key));
        assert_eq!(reason, "Incorrect API key provided: [REDACTED].");
        let long = "x".repeat(5000);
        let reason = provider_reason(&long, None);
        assert_eq!(reason.chars().count(), MAX_REASON_CHARS);
        assert!(reason.ends_with('…'));
        let rejected = classify_status(400, &reason).to_string();
        assert!(rejected.chars().count() <= 200, "{rejected}");
    }

    #[test]
    fn providers_parse_and_know_their_defaults() {
        for name in crate::core::settings::REDATOR_PROVIDERS {
            assert_eq!(Provider::parse(name).expect("provider").as_str(), *name);
        }
        assert_eq!(Provider::OpenAiCompatible.default_base_url(), None);
        assert!(!Provider::OpenAiCompatible.requires_api_key());
        assert!(Provider::Gemini.requires_api_key());
    }
}
