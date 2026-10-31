//! Anthropic: `POST {base_url}/v1/messages` com uma tool forçada cujo `input_schema` é o
//! esquema da saída; o objeto vem no `input` do bloco `tool_use`.

use serde_json::json;

use super::{Endpoint, LlmClient, LlmError, LlmRequest, LlmResponse, post_json, token_count};

const API_VERSION: &str = "2023-06-01";

#[derive(Clone)]
pub struct Anthropic {
    endpoint: Endpoint,
}

impl Anthropic {
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }
}

impl LlmClient for Anthropic {
    async fn complete_json(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/v1/messages", self.endpoint.base_url);
        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_output_tokens,
            "system": request.system,
            "messages": [{ "role": "user", "content": request.user }],
            "tools": [{
                "name": request.schema_name,
                "description": "Entrega o resultado do Redator.",
                "input_schema": request.schema,
            }],
            "tool_choice": { "type": "tool", "name": request.schema_name },
        });
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        let key = self
            .endpoint
            .api_key
            .as_ref()
            .map(|key| key.as_str().to_owned())
            .unwrap_or_default();
        let headers = [
            ("x-api-key", key.as_str()),
            ("anthropic-version", API_VERSION),
        ];
        let response = post_json(&self.endpoint, &url, &headers, &body).await?;
        let content = response["content"]
            .as_array()
            .and_then(|blocks| {
                blocks
                    .iter()
                    .find(|block| block["type"] == "tool_use" && block["input"].is_object())
            })
            .map(|block| block["input"].clone())
            .ok_or(LlmError::InvalidResponse)?;
        Ok(LlmResponse {
            content,
            input_tokens: token_count(&response["usage"]["input_tokens"]),
            output_tokens: token_count(&response["usage"]["output_tokens"]),
            cost_usd: None,
        })
    }
}
