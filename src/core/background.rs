//! Tarefas periódicas do servidor: `last_used_at` em lote a cada 10 s (D-007), contadores das
//! memórias a cada 5 s (004 D-004), a retenção a cada hora (008 Q-007, inclui a idempotência
//! com mais de 24 h), a medição da réplica do Litestream a cada 60 s (008 Q-009) e o
//! agendador da consolidação a cada 60 s (009 Q-010).

use std::time::Duration;

use tokio::task::JoinHandle;

use super::Core;
use crate::store::Store;

const TOUCH_INTERVAL: Duration = Duration::from_secs(10);
const COUNTERS_INTERVAL: Duration = Duration::from_secs(5);
const RETENTION_INTERVAL: Duration = Duration::from_secs(3600);
const BACKUP_INTERVAL: Duration = Duration::from_secs(60);
const CONSOLIDATION_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn<S: Store>(core: Core<S>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut touch = tokio::time::interval(TOUCH_INTERVAL);
        let mut counters = tokio::time::interval(COUNTERS_INTERVAL);
        let mut retention = tokio::time::interval(RETENTION_INTERVAL);
        let mut backup = tokio::time::interval(BACKUP_INTERVAL);
        let mut consolidation = tokio::time::interval(CONSOLIDATION_INTERVAL);
        loop {
            tokio::select! {
                _ = touch.tick() => {
                    if let Err(error) = core.flush_usage().await {
                        tracing::warn!(event = "tokens.touch_failed", error = %error, "falha ao gravar last_used_at");
                    }
                }
                _ = counters.tick() => {
                    if let Err(error) = core.flush_counters().await {
                        tracing::warn!(event = "memories.counters_failed", error = %error, "falha ao gravar os contadores das memorias");
                    }
                }
                _ = retention.tick() => {
                    if let Err(error) = core.retention_tick().await {
                        tracing::warn!(event = "retention.failed", error = %error, "falha ao aplicar a retencao");
                    }
                }
                _ = backup.tick() => core.sample_backup().await,
                _ = consolidation.tick() => {
                    if let Err(error) = core.consolidation_tick(super::ids::now()).await {
                        tracing::warn!(event = "consolidation.schedule_failed", error = %error, "falha no agendador da consolidacao");
                    }
                }
            }
        }
    })
}
