//! `get_status`: versão, banco, migrações, backup, contagens, fila e Redator (o completo vem na
//! 008).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    Core,
    model::{IdentityCounts, JobCount, LlmActivity},
    principal::Principal,
    redator::{self, RedatorConfig},
    settings::REDATOR_API_KEY,
};
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
    pub queue: QueueStatus,
    pub redator: RedatorStatus,
}

/// Jobs por tipo e estado.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QueueStatus {
    /// Jobs `queued` ou `running` de todos os tipos.
    #[schema(example = 3)]
    pub pending: u64,
    #[schema(example = 0)]
    pub failed: u64,
    pub by_kind: Vec<JobCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RedatorStatus {
    /// `redator.enabled`.
    pub enabled: bool,
    /// Provedor, modelo, URL e chave presentes.
    pub configured: bool,
    /// Chaves que faltam para o Redator chamar o LLM.
    #[schema(example = json!(["redator.model"]))]
    pub missing: Vec<String>,
    #[schema(example = "openai_compatible")]
    pub provider: Option<String>,
    #[schema(example = "gpt-4o-mini")]
    pub model: Option<String>,
    /// Últimos resultados das chamadas do job `redact`.
    pub activity: LlmActivity,
}

impl<S: Store> Core<S> {
    pub async fn get_status(&self, principal: &Principal) -> Result<StatusView, ApiError> {
        self.authorize(principal, "get_status")?;
        let ready = self.store().ready().await?;
        let counts = self.store().identity_counts().await?;
        let backup = doctor::backup_check(self.database()).await;
        let by_kind = self.store().job_counts().await?;
        let count = |states: &[&str]| -> u64 {
            by_kind
                .iter()
                .filter(|count| states.contains(&count.state.as_str()))
                .map(|count| count.count)
                .sum()
        };
        let queue = QueueStatus {
            pending: count(&["queued", "running"]),
            failed: count(&["failed"]),
            by_kind: by_kind.clone(),
        };
        let config = RedatorConfig::new(
            &self.settings_snapshot(),
            None,
            self.secret(REDATOR_API_KEY).await?,
        );
        let missing = config.missing();
        let redator = RedatorStatus {
            enabled: config.enabled,
            configured: missing.is_empty(),
            missing: missing.iter().map(|key| (*key).to_owned()).collect(),
            provider: config.provider.map(|provider| provider.as_str().to_owned()),
            model: config.model.clone(),
            activity: self
                .store()
                .llm_activity(redator::JOB_KIND.to_owned())
                .await?,
        };
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
            queue,
            redator,
        })
    }
}
