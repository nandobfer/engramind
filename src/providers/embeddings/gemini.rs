//! Gemini: `POST {base_url}/v1beta/models/{model}:batchEmbedContents` com `x-goog-api-key`.
//! Não oferece listagem de modelos de embedding (`PROVIDER_LISTING_UNSUPPORTED`).

use serde_json::{Value, json};

use super::{EmbedError, EmbedFuture, Embedder, Endpoint, vector};
use crate::providers::llm::post_json;

#[derive(Clone)]
pub struct Gemini {
    endpoint: Endpoint,
    dimensions: Option<usize>,
}

impl Gemini {
    pub fn new(endpoint: Endpoint, dimensions: Option<usize>) -> Self {
        Self {
            endpoint,
            dimensions,
        }
    }
}

fn parse(body: &Value, count: usize) -> Result<Vec<Vec<f32>>, EmbedError> {
    let vectors: Vec<Vec<f32>> = body["embeddings"]
        .as_array()
        .ok_or(EmbedError::InvalidResponse)?
        .iter()
        .map(|item| vector(&item["values"]).ok_or(EmbedError::InvalidResponse))
        .collect::<Result<_, _>>()?;
    if vectors.len() != count {
        return Err(EmbedError::InvalidResponse);
    }
    Ok(vectors)
}

impl Embedder for Gemini {
    fn embed<'a>(&'a self, model: &'a str, inputs: &'a [String]) -> EmbedFuture<'a> {
        Box::pin(async move {
            let model = model.strip_prefix("models/").unwrap_or(model);
            let url = format!(
                "{}/v1beta/models/{model}:batchEmbedContents",
                self.endpoint.base_url
            );
            let requests: Vec<Value> = inputs
                .iter()
                .map(|text| {
                    let mut request = json!({
                        "model": format!("models/{model}"),
                        "content": { "parts": [{ "text": text }] },
                    });
                    if let Some(dimensions) = self.dimensions {
                        request["outputDimensionality"] = json!(dimensions);
                    }
                    request
                })
                .collect();
            let key = self
                .endpoint
                .api_key
                .as_ref()
                .map(|key| key.as_str().to_owned())
                .unwrap_or_default();
            let headers = [("x-goog-api-key", key.as_str())];
            let response = post_json(
                &self.endpoint,
                &url,
                &headers,
                &json!({ "requests": requests }),
            )
            .await?;
            parse(&response, inputs.len())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_read_in_order() {
        let body = json!({ "embeddings": [{ "values": [1.0, 0.0] }, { "values": [0.5, 0.5] }] });
        assert_eq!(
            parse(&body, 2).expect("vectors"),
            vec![vec![1.0, 0.0], vec![0.5, 0.5]]
        );
        assert_eq!(parse(&body, 1), Err(EmbedError::InvalidResponse));
    }
}
