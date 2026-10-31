//! `get_status` (008 FR-012): versão, uptime, banco, backup, contagens, fila, Redator,
//! embeddings, webhooks e retenção; e o instantâneo dos gauges de `/metrics`.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    Core,
    embeddings::EmbeddingStatus,
    ids,
    model::{IdentityCounts, JobCount, LlmActivity, LlmSummary},
    principal::{Principal, Scope, TokenKind},
    redator::{self, RedatorConfig},
    retention::RetentionReport,
    settings::{REDATOR_API_KEY, WEBHOOKS_ENABLED},
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
    /// Tamanho do arquivo do banco (`page_count × page_size`).
    #[schema(example = 10_485_760)]
    pub size_bytes: u64,
}

/// Uma medição da réplica do Litestream (amostrador a cada 60 s, 008 Q-009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSample {
    pub status: CheckStatus,
    pub detail: String,
    pub latest: Option<DateTime<Utc>>,
    pub checked_at: DateTime<Utc>,
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
    /// Segundos desde a réplica mais recente, em `checked_at`; `null` sem réplica.
    #[schema(example = 4)]
    pub replica_age_seconds: Option<i64>,
    #[schema(example = "2026-10-30T12:00:00.000Z")]
    pub last_replica_at: Option<String>,
    /// Quando a réplica foi medida (a cada 60 s).
    #[schema(example = "2026-10-30T12:00:04.000Z")]
    pub checked_at: String,
}

/// Entregas de eventos (008 FR-012).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhooksStatus {
    /// `webhooks.enabled`.
    pub enabled: bool,
    /// Apps ativos com URL e segredo configurados.
    #[schema(example = 2)]
    pub apps: u64,
    /// Jobs `webhook` `queued` ou `running`.
    #[schema(example = 0)]
    pub pending: u64,
    #[schema(example = 310)]
    pub delivered_24h: u64,
    #[schema(example = 0)]
    pub failed_24h: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StatusView {
    #[schema(example = "0.8.0")]
    pub version: String,
    /// Segundos desde a subida do processo.
    #[schema(example = 86_400)]
    pub uptime_seconds: u64,
    pub database: DatabaseStatus,
    pub backup: BackupStatus,
    pub counts: IdentityCounts,
    pub queue: QueueStatus,
    pub redator: RedatorStatus,
    pub embedding: EmbeddingStatus,
    pub webhooks: WebhooksStatus,
    /// Última execução da retenção; `null` antes da primeira.
    pub retention: Option<RetentionReport>,
}

/// Valores dos gauges de `/metrics`, lidos a cada scrape.
#[derive(Debug, Clone, Default)]
pub struct MetricsSnapshot {
    pub ready: bool,
    pub jobs: Vec<JobCount>,
    pub database_size_bytes: Option<u64>,
    pub replica_age_seconds: Option<i64>,
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
    /// Chamadas do job `redact` nas últimas 24 h.
    pub last_24h: LlmSummary,
}

impl<S: Store> Core<S> {
    pub async fn get_status(&self, principal: &Principal) -> Result<StatusView, ApiError> {
        self.authorize(principal, "get_status")?;
        let ready = self.store().ready().await?;
        let counts = self.store().identity_counts().await?;
        let backup = self.backup_view().await;
        let since = ids::timestamp(ids::now() - Duration::hours(24));
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
            last_24h: self
                .store()
                .llm_summary(redator::JOB_KIND.to_owned(), since.clone())
                .await?,
        };
        let webhook_counts = self.store().webhook_counts(since).await?;
        let directory = self.store().webhook_directory().await?;
        let webhooks = WebhooksStatus {
            enabled: self.settings_snapshot().boolean(WEBHOOKS_ENABLED),
            apps: directory
                .apps
                .iter()
                .filter(|app| app.webhook && !app.disabled)
                .count() as u64,
            pending: webhook_counts.pending,
            delivered_24h: webhook_counts.delivered,
            failed_24h: webhook_counts.failed,
        };
        Ok(StatusView {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            uptime_seconds: self.started().elapsed().as_secs(),
            database: DatabaseStatus {
                status: if ready.migrations.is_current() {
                    "ok".to_owned()
                } else {
                    "migrations_pending".to_owned()
                },
                migrations: ready.migrations,
                size_bytes: self.store().database_size().await?,
            },
            backup,
            counts,
            queue,
            redator,
            embedding: self.embedding_status().await?,
            webhooks,
            retention: self.retention_report().await?,
        })
    }

    /// Mede a réplica agora e guarda o resultado (tarefa periódica).
    pub async fn sample_backup(&self) {
        let (check, latest) = doctor::backup_measure(self.database()).await;
        if check.status == CheckStatus::Fail {
            tracing::warn!(event = "backup.sample_failed", detail = %check.detail, "medicao da replica falhou");
        }
        self.set_backup_sample(BackupSample {
            status: check.status,
            detail: check.detail,
            latest,
            checked_at: Utc::now(),
        });
    }

    /// A última medição; sem nenhuma (antes da primeira rodada), mede agora.
    async fn backup_sample_or_measure(&self) -> BackupSample {
        if let Some(sample) = self.backup_sample() {
            return sample;
        }
        self.sample_backup().await;
        self.backup_sample().unwrap_or(BackupSample {
            status: CheckStatus::Fail,
            detail: "medicao indisponivel".to_owned(),
            latest: None,
            checked_at: Utc::now(),
        })
    }

    async fn backup_view(&self) -> BackupStatus {
        let sample = self.backup_sample_or_measure().await;
        BackupStatus {
            enabled: self.database().litestream_enabled,
            status: match sample.status {
                CheckStatus::Ok => "ok",
                CheckStatus::Fail => "fail",
                CheckStatus::Skip => "skip",
            }
            .to_owned(),
            detail: sample.detail,
            replica_age_seconds: sample
                .latest
                .map(|latest| (sample.checked_at - latest).num_seconds().max(0)),
            last_replica_at: sample.latest.map(ids::timestamp),
            checked_at: ids::timestamp(sample.checked_at),
        }
    }

    /// Autoriza `/metrics` (008 D-002: token `admin` com `read`, fora do catálogo) e lê os gauges.
    pub async fn metrics_snapshot(
        &self,
        principal: &Principal,
    ) -> Result<MetricsSnapshot, ApiError> {
        if principal.kind != TokenKind::Admin || !principal.scopes.contains(Scope::Read) {
            return Err(ApiError::new(
                "SCOPE_REQUIRED",
                "As metricas exigem um token admin com o escopo read",
            )
            .with_details(serde_json::json!({
                "required_kind": "admin",
                "required_scope": "read",
            })));
        }
        let ready = self
            .store()
            .ready()
            .await
            .is_ok_and(|ready| ready.migrations.is_current());
        let sample = self.backup_sample();
        Ok(MetricsSnapshot {
            ready,
            jobs: self.store().job_counts().await.unwrap_or_default(),
            database_size_bytes: self.store().database_size().await.ok(),
            replica_age_seconds: sample.and_then(|sample| {
                sample
                    .latest
                    .map(|latest| (sample.checked_at - latest).num_seconds().max(0))
            }),
        })
    }
}
