//! Provedores de embedding (desenho §11, 007): `local` (`fastembed` + ONNX Runtime) e os
//! remotos OpenAI-compatível, Ollama e Gemini. Só modelos de embedding; o `core` aplica os
//! prefixos, confere a dimensão e decide o tempo limite.

mod gemini;
mod hash;
pub mod local;
mod ollama;
mod openai_compatible;

use std::{future::Future, pin::Pin};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

pub use self::{
    gemini::Gemini, hash::HashEmbedder, local::LocalEmbedder, ollama::Ollama,
    openai_compatible::OpenAiCompatible,
};
pub use super::llm::Endpoint;
use super::llm::{LlmError, PostError};

pub type EmbedFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>, EmbedError>> + Send + 'a>>;

/// Gera um vetor por texto, na mesma ordem. Os textos já chegam com o prefixo do modelo.
pub trait Embedder: Send + Sync {
    fn embed<'a>(&'a self, model: &'a str, inputs: &'a [String]) -> EmbedFuture<'a>;
}

/// Falhas sem payload do provedor (que pode ecoar o texto enviado).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EmbedError {
    #[error("tempo limite")]
    Timeout,
    #[error("limite do provedor (HTTP 429)")]
    RateLimited,
    #[error("provedor indisponivel")]
    Unavailable,
    #[error("ONNX Runtime indisponivel ({0})")]
    Runtime(String),
    #[error("recusado pelo provedor ({0})")]
    Rejected(String),
    #[error("resposta invalida")]
    InvalidResponse,
    #[error("modelo local desconhecido: {0}")]
    UnknownModel(String),
    #[error("dimensao divergente: esperada {expected}, recebida {got}")]
    Dimension { expected: usize, got: usize },
}

impl EmbedError {
    /// Nome curto para logs e `details.outcome`.
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::Runtime(_) => "runtime_unavailable",
            Self::Rejected(_) => "rejected",
            Self::InvalidResponse => "invalid_response",
            Self::UnknownModel(_) => "unknown_model",
            Self::Dimension { .. } => "dimension_mismatch",
        }
    }
}

impl From<LlmError> for EmbedError {
    fn from(error: LlmError) -> Self {
        match error {
            LlmError::Timeout => Self::Timeout,
            LlmError::RateLimited => Self::RateLimited,
            LlmError::Unavailable => Self::Unavailable,
            LlmError::Rejected(reason) => Self::Rejected(reason),
            LlmError::InvalidResponse => Self::InvalidResponse,
        }
    }
}

impl From<PostError> for EmbedError {
    fn from(error: PostError) -> Self {
        Self::from(LlmError::from(error))
    }
}

/// Provedores aceitos em `embedding.provider` (além de `off`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProvider {
    Local,
    OpenAiCompatible,
    Ollama,
    Gemini,
}

impl EmbeddingProvider {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "openai_compatible" => Some(Self::OpenAiCompatible),
            "ollama" => Some(Self::Ollama),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::OpenAiCompatible => "openai_compatible",
            Self::Ollama => "ollama",
            Self::Gemini => "gemini",
        }
    }

    /// URL oficial; OpenAI-compatível e Ollama exigem a `base_url`.
    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Gemini => Some("https://generativelanguage.googleapis.com"),
            Self::Local | Self::OpenAiCompatible | Self::Ollama => None,
        }
    }

    pub fn requires_base_url(self) -> bool {
        matches!(self, Self::OpenAiCompatible | Self::Ollama)
    }

    /// Só o Gemini exige chave; o OpenAI-compatível aceita servidores locais sem chave.
    pub fn requires_api_key(self) -> bool {
        matches!(self, Self::Gemini)
    }
}

/// Um modelo listado por `list_embedding_models`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EmbeddingModelEntry {
    #[schema(example = "multilingual-e5-small")]
    pub id: String,
    /// `null` quando o provedor não informa.
    #[schema(example = 384)]
    pub dimensions: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Lista os modelos do provedor remoto (`GET /models` ou `GET /api/tags`).
pub async fn list_remote_models(
    provider: EmbeddingProvider,
    endpoint: &Endpoint,
) -> Result<Vec<EmbeddingModelEntry>, EmbedError> {
    match provider {
        EmbeddingProvider::OpenAiCompatible => openai_compatible::list_models(endpoint).await,
        EmbeddingProvider::Ollama => ollama::list_models(endpoint).await,
        EmbeddingProvider::Local | EmbeddingProvider::Gemini => Ok(Vec::new()),
    }
}

/// Lista de vetores de um array JSON de números; `None` se algum item não for número.
fn vector(value: &serde_json::Value) -> Option<Vec<f32>> {
    value
        .as_array()?
        .iter()
        .map(|number| number.as_f64().map(|number| number as f32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_parse_and_know_their_requirements() {
        for name in ["local", "openai_compatible", "ollama", "gemini"] {
            assert_eq!(
                EmbeddingProvider::parse(name).expect("provider").as_str(),
                name
            );
        }
        assert_eq!(EmbeddingProvider::parse("off"), None);
        assert!(EmbeddingProvider::Ollama.requires_base_url());
        assert!(!EmbeddingProvider::Gemini.requires_base_url());
        assert!(EmbeddingProvider::Gemini.requires_api_key());
        assert!(!EmbeddingProvider::OpenAiCompatible.requires_api_key());
    }

    #[test]
    fn vectors_need_numbers() {
        assert_eq!(vector(&serde_json::json!([1, 0.5])), Some(vec![1.0, 0.5]));
        assert_eq!(vector(&serde_json::json!([1, "a"])), None);
    }
}
