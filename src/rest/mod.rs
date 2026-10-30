//! Adaptador REST (`/v1`): handlers finos que traduzem HTTP para o `core` e de volta.

pub mod audit;
pub mod docs;
pub mod identities;
pub mod settings;
pub mod status;

use axum::{
    Json,
    extract::FromRequestParts,
    http::{HeaderName, StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{core::idempotency::Mutation, error::ApiError};

pub const IDEMPOTENCY_KEY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");

/// Cabeçalho `Idempotency-Key` opcional das mutações.
pub struct IdempotencyKey(pub Option<String>);

impl IdempotencyKey {
    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl<S> FromRequestParts<S> for IdempotencyKey
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        match parts.headers.get(&IDEMPOTENCY_KEY_HEADER) {
            None => Ok(Self(None)),
            Some(value) => value
                .to_str()
                .map(|value| Self(Some(value.to_owned())))
                .map_err(|_| ApiError::validation("Idempotency-Key deve ser ASCII")),
        }
    }
}

/// Resposta de uma mutação com o status da operação (a repetição usa o mesmo status).
pub fn mutation<T: Serialize>(status: StatusCode, mutation: Mutation<T>) -> Response {
    (status, Json(mutation.value)).into_response()
}
