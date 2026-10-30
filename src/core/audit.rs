//! Leitura da auditoria (`get_audit`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{
    Core,
    model::{AuditEvent, AuditFilter},
    pagination::PageRequest,
    principal::Principal,
};
use crate::{error::ApiError, store::Store};

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct AuditQuery {
    /// Entidade: `app`, `agent`, `token` ou `settings`.
    pub entity: Option<String>,
    /// Id da entidade (ou chave de configuração).
    pub entity_id: Option<String>,
    /// Ação, por exemplo `token.revoke`.
    pub action: Option<String>,
    pub cursor: Option<String>,
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuditList {
    pub events: Vec<AuditEvent>,
    pub next_cursor: Option<String>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

impl<S: Store> Core<S> {
    pub async fn get_audit(
        &self,
        principal: &Principal,
        query: AuditQuery,
    ) -> Result<AuditList, ApiError> {
        self.authorize(principal, "get_audit")?;
        let page = PageRequest::new(query.cursor.as_deref(), query.limit)?;
        if let Some(after) = &page.after
            && after.id.parse::<i64>().is_err()
        {
            return Err(ApiError::validation("cursor invalido"));
        }
        let filter = AuditFilter {
            entity: non_empty(query.entity),
            entity_id: non_empty(query.entity_id),
            action: non_empty(query.action),
        };
        let page = self.store().list_audit(filter, page).await?;
        Ok(AuditList {
            next_cursor: page.next.as_ref().map(super::pagination::Cursor::encode),
            events: page.items,
        })
    }
}
