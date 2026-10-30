//! Tarefas periódicas do servidor: `last_used_at` em lote a cada 10 s (D-007) e limpeza da
//! idempotência com mais de 24 h a cada hora (D-008).

use std::time::Duration;

use tokio::task::JoinHandle;

use super::Core;
use crate::store::Store;

const TOUCH_INTERVAL: Duration = Duration::from_secs(10);
const PURGE_INTERVAL: Duration = Duration::from_secs(3600);

pub fn spawn<S: Store>(core: Core<S>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut touch = tokio::time::interval(TOUCH_INTERVAL);
        let mut purge = tokio::time::interval(PURGE_INTERVAL);
        loop {
            tokio::select! {
                _ = touch.tick() => {
                    if let Err(error) = core.flush_usage().await {
                        tracing::warn!(event = "tokens.touch_failed", error = %error, "falha ao gravar last_used_at");
                    }
                }
                _ = purge.tick() => match core.purge_idempotency().await {
                    Ok(removed) if removed > 0 => tracing::debug!(
                        event = "idempotency.purged",
                        removed,
                        "chaves de idempotencia expiradas removidas"
                    ),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(
                        event = "idempotency.purge_failed",
                        error = %error,
                        "falha ao limpar a idempotencia"
                    ),
                },
            }
        }
    })
}
