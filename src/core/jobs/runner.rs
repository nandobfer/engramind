//! Runner de um tipo de job: captura com lease, executa até `concurrency` em paralelo e grava
//! o resultado (FR-002 a FR-004).

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::{sync::Semaphore, task::JoinHandle};

use super::JobOutcome;
use crate::{
    core::{
        Core, consolidation, embeddings, ids,
        model::{Job, JobFinish},
        redator, webhooks,
    },
    store::Store,
};

/// Espera máxima entre duas olhadas na fila sem aviso de commit.
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Concorrência do `redact` (006 D-001).
pub const REDACT_CONCURRENCY: usize = 2;
/// Concorrência do `embed` (007 Q-011); o provedor local serializa as chamadas de qualquer
/// forma, e o ONNX Runtime já usa vários threads.
pub const EMBED_CONCURRENCY: usize = 2;

/// Um tipo de job e quem o executa.
pub trait JobHandler<S: Store>: Send + Sync + 'static {
    const KIND: &'static str;
    /// Tempo que o executor reserva para um job antes de outro poder capturá-lo.
    fn lease(&self, core: &Core<S>) -> Duration;
    fn run(&self, core: &Core<S>, job: Job) -> impl Future<Output = JobOutcome> + Send;
    /// Espera antes da próxima tentativa; padrão = o do Redator.
    fn backoff(&self, attempts: i64) -> Duration {
        redator::backoff(attempts)
    }
}

/// Job `redact`: o Redator.
pub struct Redact;

impl<S: Store> JobHandler<S> for Redact {
    const KIND: &'static str = redator::JOB_KIND;

    fn lease(&self, core: &Core<S>) -> Duration {
        core.redact_lease()
    }

    async fn run(&self, core: &Core<S>, job: Job) -> JobOutcome {
        core.run_redact(&job).await
    }
}

/// Job `embed`: o vetor de uma memória ou de um lote da reindexação.
pub struct Embed;

impl<S: Store> JobHandler<S> for Embed {
    const KIND: &'static str = embeddings::JOB_KIND;

    fn lease(&self, core: &Core<S>) -> Duration {
        core.embed_lease()
    }

    async fn run(&self, core: &Core<S>, job: Job) -> JobOutcome {
        core.run_embed(&job).await
    }
}

/// Job `webhook`: a entrega de um evento a um app (008).
pub struct Webhook;

impl<S: Store> JobHandler<S> for Webhook {
    const KIND: &'static str = webhooks::JOB_KIND;

    fn lease(&self, core: &Core<S>) -> Duration {
        core.webhook_lease()
    }

    async fn run(&self, core: &Core<S>, job: Job) -> JobOutcome {
        core.run_webhook(&job).await
    }

    fn backoff(&self, attempts: i64) -> Duration {
        webhooks::backoff(attempts)
    }
}

/// Job `consolidate`: a consolidação de um canal (009).
pub struct Consolidate;

impl<S: Store> JobHandler<S> for Consolidate {
    const KIND: &'static str = consolidation::JOB_KIND;

    fn lease(&self, _core: &Core<S>) -> Duration {
        consolidation::job::LEASE
    }

    async fn run(&self, core: &Core<S>, job: Job) -> JobOutcome {
        core.run_consolidate(&job).await
    }
}

fn at(instant: DateTime<Utc>) -> String {
    ids::timestamp(instant)
}

/// Captura um job pronto em `now`; `None` com a fila vazia (ou o banco indisponível, com
/// `WARN`).
async fn claim<S: Store, H: JobHandler<S>>(
    core: &Core<S>,
    handler: &H,
    now: DateTime<Utc>,
) -> Option<Job> {
    let lease = chrono::Duration::from_std(handler.lease(core)).unwrap_or(chrono::Duration::MAX);
    let lease_until = now
        .checked_add_signed(lease)
        .unwrap_or(DateTime::<Utc>::MAX_UTC);
    match core
        .store()
        .claim_job(H::KIND.to_owned(), at(now), at(lease_until))
        .await
    {
        Ok(job) => job,
        Err(error) => {
            tracing::warn!(event = "jobs.claim_failed", kind = H::KIND, error = %error, "falha ao capturar job");
            None
        }
    }
}

/// Executa um job capturado e grava como terminou.
async fn execute<S: Store, H: JobHandler<S>>(core: &Core<S>, handler: &H, job: Job) {
    let id = job.id;
    let attempts = job.attempts;
    let outcome = handler.run(core, job).await;
    let now = ids::now();
    let finish = match &outcome {
        JobOutcome::Done(outcome) => JobFinish::Done { outcome },
        JobOutcome::Superseded => JobFinish::Done {
            outcome: "superseded",
        },
        JobOutcome::Retry(error) => {
            let delay = chrono::Duration::from_std(handler.backoff(attempts))
                .unwrap_or(chrono::Duration::MAX);
            JobFinish::Retry {
                error: error.clone(),
                run_after: at(now
                    .checked_add_signed(delay)
                    .unwrap_or(DateTime::<Utc>::MAX_UTC)),
            }
        }
        JobOutcome::Fail(error) => JobFinish::Fail {
            error: error.clone(),
        },
    };
    match core.store().finish_job(id, finish, at(now)).await {
        Ok(true) => {}
        Ok(false) => tracing::warn!(
            event = "jobs.lease_lost",
            kind = H::KIND,
            job_id = id,
            "job retomado por outro executor depois do lease; resultado descartado"
        ),
        Err(error) => tracing::warn!(
            event = "jobs.finish_failed",
            kind = H::KIND,
            job_id = id,
            error = %error,
            "falha ao gravar o fim do job; o lease vence e ele volta a fila"
        ),
    }
    match outcome {
        JobOutcome::Fail(error) => {
            crate::metrics::job_failed(H::KIND);
            tracing::error!(
            event = "jobs.failed",
            kind = H::KIND,
            job_id = id,
            attempts,
            error = %error,
            "job falhou definitivamente"
            );
        }
        JobOutcome::Superseded => tracing::debug!(
            event = "jobs.superseded",
            kind = H::KIND,
            job_id = id,
            "job de uma versao superada descartado"
        ),
        _ => {}
    }
}

/// Executa, um por vez, todos os jobs do tipo prontos em `now`; devolve quantos.
pub async fn drain<S: Store, H: JobHandler<S>>(
    core: &Core<S>,
    handler: &H,
    now: DateTime<Utc>,
) -> usize {
    let mut processed = 0;
    while let Some(job) = claim(core, handler, now).await {
        execute(core, handler, job).await;
        processed += 1;
    }
    processed
}

/// Runner em segundo plano: até `concurrency` jobs ao mesmo tempo.
pub fn spawn<S: Store, H: JobHandler<S>>(
    core: Core<S>,
    handler: H,
    concurrency: usize,
) -> JoinHandle<()> {
    let handler = Arc::new(handler);
    let permits = Arc::new(Semaphore::new(concurrency.max(1)));
    let notify = core.jobs_notify();
    tokio::spawn(async move {
        loop {
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return;
            };
            // Registra o interesse antes de olhar a fila, para não perder um aviso no meio.
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match claim(&core, handler.as_ref(), ids::now()).await {
                Some(job) => {
                    let (core, handler) = (core.clone(), handler.clone());
                    tokio::spawn(async move {
                        execute(&core, handler.as_ref(), job).await;
                        drop(permit);
                    });
                }
                None => {
                    drop(permit);
                    let _ = tokio::time::timeout(POLL_INTERVAL, notified).await;
                }
            }
        }
    })
}

impl<S: Store> Core<S> {
    /// Sobe os runners da fila (`redact`, `embed`, `webhook` e `consolidate`).
    pub fn spawn_workers(&self) -> Vec<JoinHandle<()>> {
        vec![
            spawn(self.clone(), Redact, REDACT_CONCURRENCY),
            spawn(self.clone(), Embed, EMBED_CONCURRENCY),
            spawn(self.clone(), Webhook, webhooks::WEBHOOK_CONCURRENCY),
            spawn(
                self.clone(),
                Consolidate,
                consolidation::job::CONSOLIDATE_CONCURRENCY,
            ),
        ]
    }

    /// Executa agora os jobs `consolidate` prontos em `now` (testes e diagnóstico).
    pub async fn run_consolidate_jobs(&self, now: DateTime<Utc>) -> usize {
        drain(self, &Consolidate, now).await
    }

    /// Executa agora os jobs `webhook` prontos em `now` (testes e diagnóstico).
    pub async fn run_webhook_jobs(&self, now: DateTime<Utc>) -> usize {
        drain(self, &Webhook, now).await
    }

    /// Executa agora os jobs `embed` prontos em `now` (testes e diagnóstico).
    pub async fn run_embed_jobs(&self, now: DateTime<Utc>) -> usize {
        drain(self, &Embed, now).await
    }

    /// Executa agora os jobs `redact` prontos em `now` (testes e diagnóstico).
    pub async fn run_redact_jobs(&self, now: DateTime<Utc>) -> usize {
        drain(self, &Redact, now).await
    }
}
