//! Job `embed` (desenho §7, 007): vetoriza `título + "\n" + texto` de uma memória (escrita) ou
//! de um lote (`memory_ids`, reindexação) numa única chamada e grava cada vetor só se a
//! memória ainda tiver o conteúdo e o título lidos e o modelo ainda for o vigente.

use super::{EmbedKind, JOB_KIND, MAX_ATTEMPTS, short};
use crate::{
    core::{
        Core, ids,
        jobs::JobOutcome,
        model::{EmbeddingState, Job, NewVector, VectorBatch},
    },
    store::Store,
};

impl<S: Store> Core<S> {
    /// Lease do job `embed`: o tempo limite da chamada mais a margem.
    pub(crate) fn embed_lease(&self) -> std::time::Duration {
        super::EmbeddingConfig::new(&self.settings_snapshot(), None).lease()
    }

    async fn mark(&self, ids: &[String], state: EmbeddingState) {
        if let Err(error) = self.store().mark_embedding(ids.to_vec(), state).await {
            tracing::warn!(event = "embeddings.mark_failed", error = %error, "falha ao gravar embedding_state");
        }
    }

    pub(crate) async fn run_embed(&self, job: &Job) -> JobOutcome {
        let single = job.payload["memory_id"].as_str().map(str::to_owned);
        let ids: Vec<String> = match &single {
            Some(id) => vec![id.clone()],
            None => job.payload["memory_ids"]
                .as_array()
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| id.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        };
        if ids.is_empty() {
            return JobOutcome::Fail("payload sem memory_id nem memory_ids".to_owned());
        }
        let reindex = job.payload["reindex"].as_str().map(str::to_owned);
        if let Some(id) = &single {
            match self
                .store()
                .newer_job_exists(JOB_KIND.to_owned(), id.clone(), job.id)
                .await
            {
                Ok(true) => return JobOutcome::Superseded,
                Ok(false) => {}
                Err(error) => return JobOutcome::Retry(short(&error.to_string())),
            }
        }
        let config = match self.embedding_config(&self.settings_snapshot()).await {
            Ok(config) => config,
            Err(error) => return JobOutcome::Retry(short(&error.message)),
        };
        if !config.enabled() {
            self.mark(&ids, EmbeddingState::Off).await;
            return JobOutcome::Done("off");
        }
        let runtime = match self.embedding_runtime(&config) {
            Ok(runtime) => runtime,
            Err(error) => {
                // Sem configuração: falha visível (retry_job ou reembed_all depois de configurar).
                self.mark(&ids, EmbeddingState::Failed).await;
                return JobOutcome::Fail(short(&error.message));
            }
        };
        let sources = match self.store().embed_sources(ids.clone()).await {
            Ok(sources) => sources,
            Err(error) => return JobOutcome::Retry(short(&error.to_string())),
        };
        if sources.is_empty() {
            self.after_reindex_batch(&runtime.model_id, reindex, job.id)
                .await;
            return JobOutcome::Superseded;
        }
        let texts: Vec<String> = sources
            .iter()
            .map(|source| format!("{}\n{}", source.title, source.text))
            .collect();
        let started = std::time::Instant::now();
        let vectors = match runtime
            .embed(&texts, EmbedKind::Document, runtime.timeout)
            .await
        {
            Ok(vectors) => vectors,
            Err(error) => {
                let definitive = matches!(
                    error,
                    crate::providers::embeddings::EmbedError::Dimension { .. }
                        | crate::providers::embeddings::EmbedError::UnknownModel(_)
                );
                if definitive || job.attempts >= MAX_ATTEMPTS {
                    self.mark(&ids, EmbeddingState::Failed).await;
                    return JobOutcome::Fail(short(&error.to_string()));
                }
                tracing::warn!(
                    event = "embeddings.retry",
                    provider = runtime.provider.as_str(),
                    outcome = error.outcome(),
                    attempt = job.attempts,
                    max_attempts = MAX_ATTEMPTS,
                    memories = ids.len(),
                    "embedding falhou; job reagendado com backoff"
                );
                return JobOutcome::Retry(short(&error.to_string()));
            }
        };
        let dims = vectors.first().map(Vec::len).unwrap_or_default();
        let batch = VectorBatch {
            model_id: runtime.model_id.clone(),
            dims,
            vectors: sources
                .into_iter()
                .zip(vectors)
                .map(|(source, embedding)| NewVector {
                    memory_id: source.id,
                    content_hash: source.content_hash,
                    title: source.title,
                    embedding,
                })
                .collect(),
            at: ids::timestamp(ids::now()),
        };
        let saved = match self.store().save_vectors(batch).await {
            Ok(saved) => saved,
            Err(error) => return JobOutcome::Retry(short(&error.to_string())),
        };
        tracing::debug!(
            event = "embeddings.completed",
            provider = runtime.provider.as_str(),
            model_id = %runtime.model_id,
            memories = saved,
            attempt = job.attempts,
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "vetores gravados"
        );
        self.after_reindex_batch(&runtime.model_id, reindex, job.id)
            .await;
        if saved == 0 && single.is_some() {
            JobOutcome::Superseded
        } else {
            JobOutcome::Done("embedded")
        }
    }

    /// Último lote da reindexação (nenhum outro pendente nem falho) → encerra.
    async fn after_reindex_batch(&self, model_id: &str, reindex: Option<String>, job_id: i64) {
        let Some(started_at) = reindex else {
            return;
        };
        match self
            .store()
            .reindex_jobs(started_at.clone(), Some(job_id))
            .await
        {
            Ok(jobs) if jobs.pending == 0 && jobs.failed == 0 => {
                self.finish_reindex(model_id, Some(started_at)).await;
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(
                event = "embeddings.reindex_state_failed",
                error = %error,
                "falha ao conferir a reindexacao"
            ),
        }
    }
}
