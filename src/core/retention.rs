//! Retenção (008 FR-014, Q-007): a cada hora a tarefa periódica remove, em lotes, o que passou
//! do prazo de `usage_log`, `llm_calls`, `audit_log`, jobs terminados e `idempotency`. O último
//! resultado fica em `schema_meta` e aparece em `get_status`.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    Core,
    idempotency::RETENTION_HOURS,
    ids,
    model::RetentionTable,
    settings::{
        RETENTION_AUDIT_DAYS, RETENTION_JOBS_DAYS, RETENTION_LLM_CALLS_DAYS, RETENTION_USAGE_DAYS,
    },
};
use crate::{
    metrics,
    store::{Store, StoreError},
};

/// Chave de `schema_meta` com o último `RetentionReport`.
pub const META_RETENTION: &str = "retention.last_run";
/// Linhas por transação: o escritor único nunca fica preso numa remoção longa.
pub const PURGE_BATCH: u32 = 5000;
/// Intervalo mínimo entre duas execuções.
pub const RETENTION_EVERY: Duration = Duration::hours(1);

/// Linhas removidas por tabela numa execução.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RetentionRemoved {
    pub usage_log: u64,
    pub llm_calls: u64,
    pub audit_log: u64,
    pub jobs: u64,
    pub idempotency: u64,
}

impl RetentionRemoved {
    pub fn total(&self) -> u64 {
        self.usage_log + self.llm_calls + self.audit_log + self.jobs + self.idempotency
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RetentionReport {
    #[schema(example = "2026-10-31T11:00:00.000Z")]
    pub last_run_at: String,
    pub removed: RetentionRemoved,
}

impl<S: Store> Core<S> {
    /// Última execução registrada; `None` antes da primeira.
    pub async fn retention_report(&self) -> Result<Option<RetentionReport>, StoreError> {
        Ok(self
            .store()
            .meta(META_RETENTION)
            .await?
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()))
    }

    /// Roda a retenção se a última execução tiver mais de 1 h; devolve o relatório quando rodou.
    pub async fn retention_tick(&self) -> Result<Option<RetentionReport>, StoreError> {
        let now = ids::now();
        if let Some(last) = self.retention_report().await?
            && let Some(at) = ids::parse_timestamp(&last.last_run_at)
            && now - at < RETENTION_EVERY
        {
            return Ok(None);
        }
        self.run_retention(now).await.map(Some)
    }

    /// Remove o que passou do prazo em `now` e grava o relatório.
    pub async fn run_retention(&self, now: DateTime<Utc>) -> Result<RetentionReport, StoreError> {
        let snapshot = self.settings_snapshot();
        let days = |key: &str| Some(Duration::days(snapshot.optional_whole(key)?));
        let plan = [
            (RetentionTable::UsageLog, days(RETENTION_USAGE_DAYS)),
            (RetentionTable::LlmCalls, days(RETENTION_LLM_CALLS_DAYS)),
            // `null`: a auditoria fica para sempre.
            (RetentionTable::AuditLog, days(RETENTION_AUDIT_DAYS)),
            (RetentionTable::Jobs, days(RETENTION_JOBS_DAYS)),
            (
                RetentionTable::Idempotency,
                Some(Duration::hours(RETENTION_HOURS)),
            ),
        ];
        let mut removed = RetentionRemoved::default();
        for (table, keep) in plan {
            let Some(keep) = keep else { continue };
            let before = ids::timestamp(now - keep);
            let mut count = 0;
            loop {
                let batch = self
                    .store()
                    .purge_before(table, before.clone(), PURGE_BATCH)
                    .await?;
                count += batch;
                if batch < u64::from(PURGE_BATCH) {
                    break;
                }
            }
            if count > 0 {
                metrics::retention_removed(table.as_str(), count);
            }
            match table {
                RetentionTable::UsageLog => removed.usage_log = count,
                RetentionTable::LlmCalls => removed.llm_calls = count,
                RetentionTable::AuditLog => removed.audit_log = count,
                RetentionTable::Jobs => removed.jobs = count,
                RetentionTable::Idempotency => removed.idempotency = count,
            }
        }
        let report = RetentionReport {
            last_run_at: ids::timestamp(now),
            removed,
        };
        let bytes =
            serde_json::to_vec(&report).map_err(|error| StoreError::Internal(error.to_string()))?;
        self.store().set_meta(META_RETENTION, &bytes).await?;
        if report.removed.total() > 0 {
            tracing::info!(
                event = "retention.purged",
                usage_log = report.removed.usage_log,
                llm_calls = report.removed.llm_calls,
                audit_log = report.removed.audit_log,
                jobs = report.removed.jobs,
                idempotency = report.removed.idempotency,
                "retencao aplicada"
            );
        }
        Ok(report)
    }
}
