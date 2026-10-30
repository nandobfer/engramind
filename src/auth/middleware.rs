//! `require_auth`: resolve o `Principal` pelo `core` e o põe nas extensions da requisição.
//! Os campos `app`, `agent` e `token_id` do span vêm daqui; o token nunca é registrado.

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{header::AUTHORIZATION, request::Parts},
    middleware::Next,
    response::Response,
};
use tracing::Span;

use crate::{AppState, core::principal::Principal, error::ApiError};

/// O `Principal` autenticado, para os handlers.
pub struct Auth(pub Principal);

impl<S> FromRequestParts<S> for Auth
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Principal>()
            .cloned()
            .map(Self)
            .ok_or_else(ApiError::unauthorized)
    }
}

pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let principal = state.core.authenticate(bearer_token(&request)).await?;
    record(&principal);
    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}

fn bearer_token(request: &Request) -> Option<&str> {
    let value = request.headers().get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token.trim())
}

fn record(principal: &Principal) {
    let span = Span::current();
    span.record("app", principal.app.slug.as_str());
    span.record("token_id", principal.token_id.as_str());
    if let Some(agent) = &principal.agent {
        span.record("agent", agent.slug.as_str());
    }
}
