//! Servidor de webhook de teste: guarda cada entrega (cabeçalhos e corpo bruto, para conferir
//! a assinatura) e responde os status roteirizados, depois `200`. Nunca usa rede externa.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

/// Segredo dos webhooks dos testes (32+ caracteres).
pub const SECRET: &str = "segredo-do-webhook-de-teste-com-40-caracteres";

#[derive(Debug, Clone)]
pub struct Delivery {
    pub headers: HeaderMap,
    pub raw: Vec<u8>,
    pub body: Value,
}

impl Delivery {
    pub fn header(&self, name: &str) -> String {
        self.headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    }

    /// Confere `Engramind-Signature` com o segredo, como o guia `integracao-gestor` ensina.
    pub fn signature_is_valid(&self, secret: &str) -> bool {
        let header = self.header("engramind-signature");
        let mut parts = header.split(',');
        let (Some(t), Some(v1)) = (
            parts.next().and_then(|part| part.strip_prefix("t=")),
            parts.next().and_then(|part| part.strip_prefix("v1=")),
        ) else {
            return false;
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(t.as_bytes());
        mac.update(b".");
        mac.update(&self.raw);
        let expected: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        expected == v1
    }
}

#[derive(Default)]
struct Inbox {
    statuses: VecDeque<u16>,
    deliveries: Vec<Delivery>,
}

#[derive(Clone)]
pub struct WebhookServer {
    pub url: String,
    inbox: Arc<Mutex<Inbox>>,
}

async fn receive(
    State(inbox): State<Arc<Mutex<Inbox>>>,
    headers: HeaderMap,
    raw: Bytes,
) -> Response {
    let status = {
        let mut inbox = inbox.lock().expect("inbox");
        inbox.deliveries.push(Delivery {
            headers,
            body: serde_json::from_slice(&raw).unwrap_or(Value::Null),
            raw: raw.to_vec(),
        });
        inbox.statuses.pop_front().unwrap_or(200)
    };
    StatusCode::from_u16(status)
        .expect("status")
        .into_response()
}

impl WebhookServer {
    pub async fn start() -> Self {
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let app = Router::new().fallback(receive).with_state(inbox.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        Self {
            url: format!("http://{address}/events"),
            inbox,
        }
    }

    /// Próximas respostas, na ordem; depois, `200`.
    pub fn respond(&self, statuses: &[u16]) {
        self.inbox
            .lock()
            .expect("inbox")
            .statuses
            .extend(statuses.iter().copied());
    }

    pub fn deliveries(&self) -> Vec<Delivery> {
        self.inbox.lock().expect("inbox").deliveries.clone()
    }

    /// Tipos dos eventos recebidos, na ordem.
    pub fn types(&self) -> Vec<String> {
        self.deliveries()
            .iter()
            .map(|delivery| {
                delivery.body["type"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }
}
