//! LLM roteirizado: um servidor HTTP local que devolve as respostas da fila, na ordem, e
//! guarda cada requisição recebida. Nunca usa rede nem provedor real.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

/// Uma resposta roteirizada.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
    pub delay: Duration,
}

/// Uma requisição recebida.
#[derive(Debug, Clone)]
pub struct Received {
    pub path: String,
    pub headers: HeaderMap,
    pub body: Value,
}

#[derive(Default)]
struct Script {
    replies: VecDeque<Reply>,
    received: Vec<Received>,
}

#[derive(Clone)]
pub struct ScriptedLlm {
    pub url: String,
    script: Arc<Mutex<Script>>,
}

async fn answer(
    State(script): State<Arc<Mutex<Script>>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let reply = {
        let mut script = script.lock().expect("script");
        script.received.push(Received {
            path: uri.path().to_owned(),
            headers,
            body: serde_json::from_slice(&body).unwrap_or(Value::Null),
        });
        script.replies.pop_front()
    };
    let Some(reply) = reply else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "roteiro vazio").into_response();
    };
    if !reply.delay.is_zero() {
        tokio::time::sleep(reply.delay).await;
    }
    (
        StatusCode::from_u16(reply.status).expect("status"),
        [("content-type", "application/json")],
        reply.body.to_string(),
    )
        .into_response()
}

impl ScriptedLlm {
    pub async fn start() -> Self {
        let script = Arc::new(Mutex::new(Script::default()));
        let app = Router::new().fallback(answer).with_state(script.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        Self {
            url: format!("http://{address}"),
            script,
        }
    }

    pub fn push(&self, status: u16, body: Value) -> &Self {
        self.push_delayed(status, body, Duration::ZERO)
    }

    pub fn push_delayed(&self, status: u16, body: Value, delay: Duration) -> &Self {
        self.script
            .lock()
            .expect("script")
            .replies
            .push_back(Reply {
                status,
                body,
                delay,
            });
        self
    }

    pub fn received(&self) -> Vec<Received> {
        self.script.lock().expect("script").received.clone()
    }
}

/// Resposta OpenAI-compatível com o objeto em `content` e o uso (com `cost`, como o
/// OpenRouter).
pub fn openai(content: Value) -> Value {
    json!({
        "choices": [{ "message": { "role": "assistant", "content": content.to_string() } }],
        "usage": { "prompt_tokens": 120, "completion_tokens": 30, "cost": 0.00042 },
    })
}

pub fn anthropic(content: Value) -> Value {
    json!({
        "content": [
            { "type": "text", "text": "ok" },
            { "type": "tool_use", "id": "toolu_1", "name": "redator_result", "input": content },
        ],
        "usage": { "input_tokens": 110, "output_tokens": 25 },
    })
}

pub fn gemini(content: Value) -> Value {
    json!({
        "candidates": [{ "content": { "parts": [{ "text": content.to_string() }] } }],
        "usageMetadata": { "promptTokenCount": 100, "candidatesTokenCount": 20 },
    })
}
