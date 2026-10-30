//! Idempotência genérica das mutações (FR-009, D-008): a chave vale por token durante 24 h; a
//! mesma chave com a mesma operação e o mesmo corpo devolve a resposta gravada, e com outro
//! corpo devolve `409 IDEMPOTENCY_KEY_CONFLICT`.

use std::future::Future;

use chrono::Duration;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    Core, actor, ids,
    model::{IdemRecord, IdemRow, WriteContext},
    operations::OperationSpec,
    principal::Principal,
};
use crate::{
    error::ApiError,
    store::{Store, StoreError},
};

pub(crate) fn key_taken() -> ApiError {
    conflict()
}

pub const RETENTION_HOURS: i64 = 24;
const MAX_KEY_CHARS: usize = 255;

/// Resultado de uma mutação; `replayed` quando veio do registro de idempotência.
#[derive(Debug, Clone, PartialEq)]
pub struct Mutation<T> {
    pub value: T,
    pub replayed: bool,
}

/// Valida a chave: 1 a 255 caracteres ASCII visíveis.
pub fn validate_key(key: &str) -> Result<(), ApiError> {
    let length = key.chars().count();
    if (1..=MAX_KEY_CHARS).contains(&length) && key.bytes().all(|byte| byte.is_ascii_graphic()) {
        Ok(())
    } else {
        Err(ApiError::validation(
            "Idempotency-Key deve ter de 1 a 255 caracteres ASCII visiveis",
        ))
    }
}

/// JSON com chaves ordenadas em todos os níveis, independente da ordem recebida.
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        Value::String(key.clone()),
                        canonical(&map[key.as_str()])
                    )
                })
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

/// SHA-256 de `operação + corpo canônico`.
pub fn request_hash(operation: &str, input: &Value) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(operation.as_bytes());
    hasher.update(b"\n");
    hasher.update(canonical(input).as_bytes());
    hasher.finalize().to_vec()
}

fn conflict() -> ApiError {
    ApiError::new(
        "IDEMPOTENCY_KEY_CONFLICT",
        "Idempotency-Key ja usada com outra operacao ou outro corpo",
    )
}

impl<S: Store> Core<S> {
    /// Executa uma mutação com auditoria e, quando há chave, com idempotência na mesma
    /// transação. A repetição é conferida antes de `run`, que resolve os alvos e grava.
    pub(crate) async fn mutate<T, F, Fut>(
        &self,
        principal: &Principal,
        operation: &OperationSpec,
        key: Option<&str>,
        input: &impl Serialize,
        status: u16,
        run: F,
    ) -> Result<Mutation<T>, ApiError>
    where
        T: DeserializeOwned,
        F: FnOnce(WriteContext) -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        let now = ids::now();
        let at = ids::timestamp(now);
        let Some(key) = key else {
            let context = WriteContext {
                actor: actor(principal),
                at,
                idempotency: None,
            };
            return run(context).await.map(|value| Mutation {
                value,
                replayed: false,
            });
        };
        validate_key(key)?;
        let input = serde_json::to_value(input).map_err(|_| ApiError::internal())?;
        let hash = request_hash(operation.name, &input);
        let expired_before = ids::timestamp(now - Duration::hours(RETENTION_HOURS));
        if let Some(replay) = self
            .replay(&principal.token_id, key, &hash, &expired_before)
            .await?
        {
            return Ok(replay);
        }
        let context = WriteContext {
            actor: actor(principal),
            at,
            idempotency: Some(IdemRecord {
                token_id: principal.token_id.clone(),
                key: key.to_owned(),
                request_hash: hash.clone(),
                status,
                expired_before: expired_before.clone(),
            }),
        };
        match run(context).await {
            Ok(value) => Ok(Mutation {
                value,
                replayed: false,
            }),
            // Outra requisição com a mesma chave gravou primeiro.
            Err(error) if error.code == "IDEMPOTENCY_KEY_CONFLICT" => self
                .replay(&principal.token_id, key, &hash, &expired_before)
                .await?
                .ok_or_else(conflict),
            Err(error) => Err(error),
        }
    }

    async fn replay<T: DeserializeOwned>(
        &self,
        token_id: &str,
        key: &str,
        hash: &[u8],
        expired_before: &str,
    ) -> Result<Option<Mutation<T>>, ApiError> {
        let row: Option<IdemRow> = self.store().find_idempotency(token_id, key).await?;
        let Some(row) = row.filter(|row| row.created_at.as_str() >= expired_before) else {
            return Ok(None);
        };
        if row.request_hash != hash {
            return Err(conflict());
        }
        let value = serde_json::from_str(&row.response_json).map_err(|_| ApiError::internal())?;
        Ok(Some(Mutation {
            value,
            replayed: true,
        }))
    }

    /// Remove registros de idempotência com mais de 24 h.
    pub async fn purge_idempotency(&self) -> Result<u64, StoreError> {
        let before = ids::timestamp(ids::now() - Duration::hours(RETENTION_HOURS));
        self.store().purge_idempotency(before).await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn hash_ignores_key_order_but_not_values_or_operation() {
        let a = json!({ "slug": "app-a", "name": "A", "nested": { "x": 1, "y": [1, 2] } });
        let b = json!({ "nested": { "y": [1, 2], "x": 1 }, "name": "A", "slug": "app-a" });
        assert_eq!(
            request_hash("create_app", &a),
            request_hash("create_app", &b)
        );
        assert_ne!(
            request_hash("create_app", &a),
            request_hash("create_agent", &a)
        );
        let c = json!({ "slug": "app-a", "name": "B", "nested": { "x": 1, "y": [1, 2] } });
        assert_ne!(
            request_hash("create_app", &a),
            request_hash("create_app", &c)
        );
        assert_eq!(canonical(&json!({"b": 1, "a": "x"})), r#"{"a":"x","b":1}"#);
    }

    #[test]
    fn keys_are_bounded_visible_ascii() {
        assert!(validate_key("pedido-42").is_ok());
        assert!(validate_key("").is_err());
        assert!(validate_key("com espaco").is_err());
        assert!(validate_key("acentuação").is_err());
        assert!(validate_key(&"k".repeat(256)).is_err());
    }
}
