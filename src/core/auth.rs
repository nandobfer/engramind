//! Autenticação: do Bearer ao `Principal`, falhando fechado (FR-001).

use std::time::Instant;

use super::{Core, ids, model::TokenStatus, principal::Principal};
use crate::{
    auth::token,
    error::ApiError,
    store::{Store, StoreError},
};

impl<S: Store> Core<S> {
    /// Confere o token e aplica o rate limit. Qualquer falha de identidade → `401 AUTH_INVALID`.
    pub async fn authenticate(&self, bearer: Option<&str>) -> Result<Principal, ApiError> {
        let bearer = bearer.ok_or_else(ApiError::unauthorized)?;
        let parsed = token::parse(bearer).map_err(|_| ApiError::unauthorized())?;
        let row = self
            .store()
            .find_auth_token(parsed.id)
            .await
            .map_err(|error| match error {
                StoreError::Unavailable => ApiError::database_unavailable(),
                other => ApiError::from(other),
            })?
            .ok_or_else(ApiError::unauthorized)?;
        // Verifica sempre o HMAC, mesmo para tokens inativos, para não revelar o estado.
        let verified = self.tokens().verify(parsed.secret, &row.digest).is_ok();
        if !verified
            || row.token.status != TokenStatus::Active
            || row.app_disabled
            || row.agent_disabled
        {
            return Err(ApiError::unauthorized());
        }
        let token = row.token;
        let scopes = super::principal::ScopeSet::from_scopes(&token.scopes)
            .map_err(|_| ApiError::internal())?;
        let principal = Principal {
            token_id: token.id,
            app: token.app,
            agent: token.agent,
            kind: token.kind,
            scopes,
        };
        let limit = self.settings_snapshot().rate_per_minute();
        self.usage()
            .hit(
                &principal.token_id,
                limit,
                Instant::now(),
                ids::timestamp(ids::now()),
            )
            .map_err(ApiError::rate_limited)?;
        Ok(principal)
    }

    /// Grava em lote os `last_used_at` acumulados.
    pub async fn flush_usage(&self) -> Result<(), StoreError> {
        let batch = self.usage().drain_last_used();
        self.store().touch_tokens(batch).await
    }
}
