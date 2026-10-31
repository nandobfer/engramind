//! OpenAI-compatível: `POST {base_url}/embeddings` com `{ model, input, dimensions? }` e
//! `GET {base_url}/models` para a listagem (OpenAI, OpenRouter, Mistral, LM Studio, vLLM…).

use serde_json::{Value, json};

use super::{EmbedError, EmbedFuture, Embedder, EmbeddingModelEntry, Endpoint, vector};
use crate::providers::llm::{get_json, post_json};

#[derive(Clone)]
pub struct OpenAiCompatible {
    endpoint: Endpoint,
    dimensions: Option<usize>,
}

impl OpenAiCompatible {
    pub fn new(endpoint: Endpoint, dimensions: Option<usize>) -> Self {
        Self {
            endpoint,
            dimensions,
        }
    }
}

fn authorization(endpoint: &Endpoint) -> Option<String> {
    endpoint
        .api_key
        .as_ref()
        .map(|key| format!("Bearer {}", key.as_str()))
}

/// Vetores de `data[]`, na ordem de `data[].index`.
fn parse(body: &Value, count: usize) -> Result<Vec<Vec<f32>>, EmbedError> {
    let data = body["data"].as_array().ok_or(EmbedError::InvalidResponse)?;
    let mut slots: Vec<Option<Vec<f32>>> = vec![None; count];
    for (position, item) in data.iter().enumerate() {
        let index = item["index"]
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(position);
        let slot = slots.get_mut(index).ok_or(EmbedError::InvalidResponse)?;
        *slot = Some(vector(&item["embedding"]).ok_or(EmbedError::InvalidResponse)?);
    }
    slots
        .into_iter()
        .map(|slot| slot.ok_or(EmbedError::InvalidResponse))
        .collect()
}

impl Embedder for OpenAiCompatible {
    fn embed<'a>(&'a self, model: &'a str, inputs: &'a [String]) -> EmbedFuture<'a> {
        Box::pin(async move {
            let url = format!("{}/embeddings", self.endpoint.base_url);
            let mut body = json!({ "model": model, "input": inputs });
            if let Some(dimensions) = self.dimensions {
                body["dimensions"] = json!(dimensions);
            }
            let authorization = authorization(&self.endpoint);
            let headers: Vec<(&str, &str)> = authorization
                .as_deref()
                .map(|value| vec![("authorization", value)])
                .unwrap_or_default();
            let response = post_json(&self.endpoint, &url, &headers, &body).await?;
            parse(&response, inputs.len())
        })
    }
}

pub(super) async fn list_models(
    endpoint: &Endpoint,
) -> Result<Vec<EmbeddingModelEntry>, EmbedError> {
    let url = format!("{}/models", endpoint.base_url);
    let authorization = authorization(endpoint);
    let headers: Vec<(&str, &str)> = authorization
        .as_deref()
        .map(|value| vec![("authorization", value)])
        .unwrap_or_default();
    let body = get_json(endpoint, &url, &headers).await?;
    let data = body["data"].as_array().ok_or(EmbedError::InvalidResponse)?;
    Ok(data
        .iter()
        .filter_map(|model| model["id"].as_str())
        .map(|id| EmbeddingModelEntry {
            id: id.to_owned(),
            dimensions: None,
            description: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_is_ordered_by_index() {
        let body = json!({ "data": [
            { "index": 1, "embedding": [0.0, 1.0] },
            { "index": 0, "embedding": [1.0, 0.0] },
        ] });
        assert_eq!(
            parse(&body, 2).expect("vectors"),
            vec![vec![1.0, 0.0], vec![0.0, 1.0]]
        );
        assert_eq!(parse(&body, 1), Err(EmbedError::InvalidResponse));
        assert_eq!(
            parse(&json!({ "data": [{ "index": 0, "embedding": [1.0] }] }), 2),
            Err(EmbedError::InvalidResponse)
        );
    }
}
