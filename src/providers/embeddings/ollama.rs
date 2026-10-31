//! Ollama: `POST {base_url}/api/embed` com `{ model, input }` e `GET {base_url}/api/tags` para
//! a listagem.

use serde_json::{Value, json};

use super::{EmbedError, EmbedFuture, Embedder, EmbeddingModelEntry, Endpoint, vector};
use crate::providers::llm::{get_json, post_json};

#[derive(Clone)]
pub struct Ollama {
    endpoint: Endpoint,
}

impl Ollama {
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }
}

fn parse(body: &Value, count: usize) -> Result<Vec<Vec<f32>>, EmbedError> {
    let vectors: Vec<Vec<f32>> = body["embeddings"]
        .as_array()
        .ok_or(EmbedError::InvalidResponse)?
        .iter()
        .map(|item| vector(item).ok_or(EmbedError::InvalidResponse))
        .collect::<Result<_, _>>()?;
    if vectors.len() != count {
        return Err(EmbedError::InvalidResponse);
    }
    Ok(vectors)
}

impl Embedder for Ollama {
    fn embed<'a>(&'a self, model: &'a str, inputs: &'a [String]) -> EmbedFuture<'a> {
        Box::pin(async move {
            let url = format!("{}/api/embed", self.endpoint.base_url);
            let body = json!({ "model": model, "input": inputs });
            let response = post_json(&self.endpoint, &url, &[], &body).await?;
            parse(&response, inputs.len())
        })
    }
}

pub(super) async fn list_models(
    endpoint: &Endpoint,
) -> Result<Vec<EmbeddingModelEntry>, EmbedError> {
    let url = format!("{}/api/tags", endpoint.base_url);
    let body = get_json(endpoint, &url, &[]).await?;
    let models = body["models"]
        .as_array()
        .ok_or(EmbedError::InvalidResponse)?;
    Ok(models
        .iter()
        .filter_map(|model| model["name"].as_str().or_else(|| model["model"].as_str()))
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
    fn embeddings_must_match_the_inputs() {
        let body = json!({ "embeddings": [[1.0, 0.0], [0.0, 1.0]] });
        assert_eq!(parse(&body, 2).expect("vectors").len(), 2);
        assert_eq!(parse(&body, 3), Err(EmbedError::InvalidResponse));
        assert_eq!(parse(&json!({}), 1), Err(EmbedError::InvalidResponse));
    }
}
