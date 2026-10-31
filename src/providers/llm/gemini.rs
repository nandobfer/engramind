//! Gemini: `POST {base_url}/v1beta/models/{model}:generateContent` com
//! `responseMimeType: application/json` e `responseSchema` (subconjunto OpenAPI: tipos em
//! maiúsculas e sem `additionalProperties`).

use serde_json::{Map, Value, json};

use super::{
    Endpoint, LlmClient, LlmError, LlmRequest, LlmResponse, parse_object, post_json, token_count,
};

#[derive(Clone)]
pub struct Gemini {
    endpoint: Endpoint,
}

impl Gemini {
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }
}

/// Converte o JSON Schema para o esquema do Gemini.
pub(super) fn response_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut converted = Map::new();
            for (key, value) in map {
                match key.as_str() {
                    "additionalProperties" => {}
                    "type" => {
                        converted.insert(
                            key.clone(),
                            value
                                .as_str()
                                .map(|kind| Value::String(kind.to_ascii_uppercase()))
                                .unwrap_or_else(|| value.clone()),
                        );
                    }
                    "properties" => {
                        let properties = value
                            .as_object()
                            .map(|properties| {
                                properties
                                    .iter()
                                    .map(|(name, item)| (name.clone(), response_schema(item)))
                                    .collect::<Map<_, _>>()
                            })
                            .unwrap_or_default();
                        converted.insert(key.clone(), Value::Object(properties));
                    }
                    _ => {
                        converted.insert(key.clone(), response_schema(value));
                    }
                }
            }
            Value::Object(converted)
        }
        other => other.clone(),
    }
}

impl LlmClient for Gemini {
    async fn complete_json(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.endpoint.base_url, request.model
        );
        let mut body = json!({
            "systemInstruction": { "parts": [{ "text": request.system }] },
            "contents": [{ "role": "user", "parts": [{ "text": request.user }] }],
            "generationConfig": {
                "maxOutputTokens": request.max_output_tokens,
                "responseMimeType": "application/json",
                "responseSchema": response_schema(&request.schema),
            },
        });
        if let Some(temperature) = request.temperature {
            body["generationConfig"]["temperature"] = json!(temperature);
        }
        let key = self
            .endpoint
            .api_key
            .as_ref()
            .map(|key| key.as_str().to_owned())
            .unwrap_or_default();
        let headers = [("x-goog-api-key", key.as_str())];
        let response = post_json(&self.endpoint, &url, &headers, &body).await?;
        let text = response["candidates"][0]["content"]["parts"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<String>()
            })
            .filter(|text| !text.is_empty())
            .ok_or(LlmError::InvalidResponse)?;
        Ok(LlmResponse {
            content: parse_object(&text)?,
            input_tokens: token_count(&response["usageMetadata"]["promptTokenCount"]),
            output_tokens: token_count(&response["usageMetadata"]["candidatesTokenCount"]),
            cost_usd: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn the_schema_drops_additional_properties_and_uppercases_types() {
        let schema = json!({
            "type": "object",
            "properties": { "title": { "type": "string" } },
            "required": ["title"],
            "additionalProperties": false,
        });
        assert_eq!(
            super::response_schema(&schema),
            json!({
                "type": "OBJECT",
                "properties": { "title": { "type": "STRING" } },
                "required": ["title"],
            })
        );
    }
}
