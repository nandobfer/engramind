//! Primeiro token `admin` a partir de `BOOTSTRAP_ADMIN_TOKEN` (FR-007): aceito só quando o banco
//! não tem nenhum token; com tokens existentes é ignorado.

use thiserror::Error;

use super::{
    Core, ids,
    model::{Actor, NewApp, NewToken, WriteContext},
    principal::{ScopeSet, TokenKind},
};
use crate::{
    auth::token,
    config::Secret,
    store::{Store, StoreError},
};

pub const BOOTSTRAP_APP_SLUG: &str = "bootstrap";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapOutcome {
    /// Token criado com este id.
    Created(String),
    /// Já havia tokens; o valor foi ignorado.
    Ignored,
    /// Sem `BOOTSTRAP_ADMIN_TOKEN` e sem nenhum token: ninguém consegue administrar.
    MissingWithEmptyDatabase,
    /// Sem `BOOTSTRAP_ADMIN_TOKEN`, com tokens existentes.
    NotConfigured,
}

#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("BOOTSTRAP_ADMIN_TOKEN invalido: use eng.tk_<8 hex>.<segredo com 32+ caracteres>")]
    InvalidFormat,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl<S: Store> Core<S> {
    pub async fn bootstrap(
        &self,
        value: Option<&Secret>,
    ) -> Result<BootstrapOutcome, BootstrapError> {
        let Some(value) = value else {
            return Ok(if self.store().count_tokens().await? == 0 {
                BootstrapOutcome::MissingWithEmptyDatabase
            } else {
                BootstrapOutcome::NotConfigured
            });
        };
        let parsed = token::parse(value.expose()).map_err(|_| BootstrapError::InvalidFormat)?;
        let app = NewApp {
            slug: BOOTSTRAP_APP_SLUG.to_owned(),
            name: "Bootstrap".to_owned(),
            description: Some("Criado pelo BOOTSTRAP_ADMIN_TOKEN".to_owned()),
            webhook_url: None,
            webhook_secret: None,
        };
        let new = NewToken {
            id: Some(parsed.id.to_owned()),
            app_id: String::new(),
            agent_id: None,
            kind: TokenKind::Admin,
            scopes: ScopeSet::ALL.scopes(),
            label: Some("bootstrap".to_owned()),
            expires_at: None,
            digest: self.tokens().digest(parsed.secret),
        };
        let context = WriteContext {
            actor: Actor::default(),
            at: ids::timestamp(ids::now()),
            idempotency: None,
            events: Vec::new(),
        };
        Ok(match self.store().bootstrap(app, new, context).await? {
            Some(token) => BootstrapOutcome::Created(token.id),
            None => BootstrapOutcome::Ignored,
        })
    }
}
