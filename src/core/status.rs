//! `get_status` básico: versão, banco, migrações, backup e contagens (o completo vem na 008).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Core, model::IdentityCounts, principal::Principal};
use crate::{
    doctor::{self, CheckStatus},
    error::ApiError,
    store::{MigrationStatus, Store},
};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DatabaseStatus {
    #[schema(example = "ok")]
    pub status: String,
    pub migrations: MigrationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BackupStatus {
    /// `LITESTREAM_ENABLED`.
    pub enabled: bool,
    /// `ok`, `fail` ou `skip`.
    #[schema(example = "ok")]
    pub status: String,
    #[schema(example = "ultima replica ha 4s (2026-10-30T12:00:00+00:00)")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StatusView {
    #[schema(example = "0.2.0")]
    pub version: String,
    pub database: DatabaseStatus,
    pub backup: BackupStatus,
    pub counts: IdentityCounts,
}

impl<S: Store> Core<S> {
    pub async fn get_status(&self, principal: &Principal) -> Result<StatusView, ApiError> {
        self.authorize(principal, "get_status")?;
        let ready = self.store().ready().await?;
        let counts = self.store().identity_counts().await?;
        let backup = doctor::backup_check(self.database()).await;
        Ok(StatusView {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            database: DatabaseStatus {
                status: if ready.migrations.is_current() {
                    "ok".to_owned()
                } else {
                    "migrations_pending".to_owned()
                },
                migrations: ready.migrations,
            },
            backup: BackupStatus {
                enabled: self.database().litestream_enabled,
                status: match backup.status {
                    CheckStatus::Ok => "ok",
                    CheckStatus::Fail => "fail",
                    CheckStatus::Skip => "skip",
                }
                .to_owned(),
                detail: backup.detail,
            },
            counts,
        })
    }
}
