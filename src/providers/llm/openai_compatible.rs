//! OpenAI-compatível: `POST {base_url}/chat/completions` com `response_format: json_schema`.
//! Servidor que recusa o `response_format` (400/422 citando o campo) recebe o pedido de novo
//! sem ele, em JSON livre que o serviço valida.

use serde_json::{Value, json};

use super::{
    Endpoint, LlmClient, LlmError, LlmRequest, LlmResponse, PostError, parse_object, post_json,
    token_count,
};

#[derive(Clone)]
pub struct OpenAiCompatible {
    endpoint: Endpoint,
}

impl OpenAiCompatible {
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }

    fn body(request: &LlmRequest, structured: bool) -> Value {
        let mut body = json!({
            "model": request.model,
            "messages": [
                { "role": "system", "content": request.system },
                { "role": "user", "content": request.user },
            ],
            "temperature": 0,
            "max_tokens": request.max_output_tokens,
        });
        if structured {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "name": request.schema_name,
                    "strict": true,
                    "schema": request.schema,
                },
            });
        }
        body
    }

    async fn send(&self, request: &LlmRequest, structured: bool) -> Result<Value, PostError> {
        let url = format!("{}/chat/completions", self.endpoint.base_url);
        let authorization = self
            .endpoint
            .api_key
            .as_ref()
            .map(|key| format!("Bearer {}", key.as_str()));
        let headers: Vec<(&str, &str)> = authorization
            .as_deref()
            .map(|value| vec![("authorization", value)])
            .unwrap_or_default();
        post_json(
            &self.endpoint,
            &url,
            &headers,
            &Self::body(request, structured),
        )
        .await
    }
}

impl LlmClient for OpenAiCompatible {
    async fn complete_json(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let body = match self.send(&request, true).await {
            Err(PostError::Http(failure))
                if matches!(failure.status, 400 | 422)
                    && failure.body.contains("response_format") =>
            {
                self.send(&request, false).await?
            }
            other => other?,
        };
        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .ok_or(LlmError::InvalidResponse)?;
        Ok(LlmResponse {
            content: parse_object(content)?,
            input_tokens: token_count(&body["usage"]["prompt_tokens"]),
            output_tokens: token_count(&body["usage"]["completion_tokens"]),
            cost_usd: body["usage"]["cost"].as_f64().filter(|cost| *cost >= 0.0),
        })
    }
}
