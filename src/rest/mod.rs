//! Adaptador REST (`/v1`): handlers finos que traduzem HTTP para o `core` e de volta.

pub mod audit;
pub mod channels;
pub mod context;
pub mod docs;
pub mod identities;
pub mod jobs;
pub mod memories;
pub mod settings;
pub mod status;
pub mod subscriptions;

use axum::{
    Json,
    extract::FromRequestParts,
    http::{HeaderName, StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{core::idempotency::Mutation, error::ApiError};

pub const IDEMPOTENCY_KEY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");
/// Filtro de canais da conexão (desenho §5); o MCP lê o mesmo cabeçalho.
pub const CHANNELS_HEADER: HeaderName = HeaderName::from_static("engramind-channels");

/// Cabeçalho `Engramind-Channels` opcional (lista de slugs e prefixos).
pub struct ChannelsHeader(pub Option<String>);

impl ChannelsHeader {
    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }

    pub fn from_headers(headers: &axum::http::HeaderMap) -> Result<Self, ApiError> {
        match headers.get(&CHANNELS_HEADER) {
            None => Ok(Self(None)),
            Some(value) => value
                .to_str()
                .map(|value| Self(Some(value.to_owned())))
                .map_err(|_| ApiError::validation("Engramind-Channels deve ser ASCII")),
        }
    }
}

impl<S> FromRequestParts<S> for ChannelsHeader
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Self::from_headers(&parts.headers)
    }
}

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
