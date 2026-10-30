use std::str::FromStr;

use axum::{
    Json,
    extract::{
        FromRequest, FromRequestParts, Path, Query, Request,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{HeaderName, request::Parts},
};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::error::ApiError;

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(map_json_rejection)
    }
}

pub struct ApiQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(map_query_rejection)
    }
}

pub struct ApiPath<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiPath<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(map_path_rejection)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RequestId(pub Uuid);

impl<S> FromRequestParts<S> for RequestId
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let value = parts
            .headers
            .get(&REQUEST_ID_HEADER)
            .and_then(|header| header.to_str().ok())
            .and_then(|header| Uuid::from_str(header).ok())
            .ok_or_else(ApiError::internal)?;
        Ok(Self(value))
    }
}

fn map_json_rejection(rejection: JsonRejection) -> ApiError {
    if rejection.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::payload_too_large()
    } else {
        ApiError::validation("Body JSON ausente ou invalido")
    }
}

fn map_query_rejection(rejection: QueryRejection) -> ApiError {
    let _ = rejection;
    ApiError::validation("Parametros de query invalidos")
}

fn map_path_rejection(rejection: PathRejection) -> ApiError {
    let _ = rejection;
    ApiError::validation("Parametro de rota invalido")
}
