//! Fila de jobs sobre a tabela `jobs` (desenho §6, decisão 0016): um runner por tipo, acordado
//! no commit e a cada segundo, com lease, backoff exponencial e descarte de jobs superados.
//! Sem broker.

pub mod runner;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use super::{
    Core,
    idempotency::Mutation,
    ids,
    model::{Job, JobFilter, JobState, LlmCall, LlmCallFilter},
    pagination::{Cursor, PageRequest},
    principal::Principal,
};
use crate::{error::ApiError, store::Store};

/// Como o handler terminou um job capturado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    /// `done` com o `outcome` informado.
    Done(&'static str),
    /// Versão superada ou memória removida: `done` com `outcome = superseded`, sem efeito.
    Superseded,
    /// Falha temporária: volta à fila com backoff.
    Retry(String),
    /// Falha definitiva: `failed` e um `ERROR`.
    Fail(String),
}

pub(crate) fn job_not_found() -> ApiError {
    ApiError::new("JOB_NOT_FOUND", "Job inexistente")
}

pub(crate) fn job_not_retryable() -> ApiError {
    ApiError::new(
        "JOB_NOT_RETRYABLE",
        "Só jobs com state = failed voltam para a fila",
    )
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct ListJobsQuery {
    /// `queued`, `running`, `done` ou `failed`.
    pub state: Option<String>,
    /// Tipo do job, por exemplo `redact`.
    pub kind: Option<String>,
    pub cursor: Option<String>,
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobList {
    pub jobs: Vec<Job>,
    pub next_cursor: Option<String>,
}

/// Página por id decrescente; o cursor precisa carregar um id numérico.
pub(crate) fn numeric_page(
    cursor: Option<&str>,
    limit: Option<u32>,
) -> Result<PageRequest, ApiError> {
    let page = PageRequest::new(cursor, limit)?;
    if let Some(after) = &page.after
        && after.id.parse::<i64>().is_err()
    {
        return Err(ApiError::validation("cursor invalido"));
    }
    Ok(page)
}

fn job_id(value: &str) -> Result<i64, ApiError> {
    value
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| {
            ApiError::validation("job deve ser o id numerico do job")
                .with_details(json!({ "field": "job" }))
        })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct ListLlmCallsQuery {
    /// Só as chamadas a partir deste instante (RFC 3339).
    pub since: Option<String>,
    /// `redact` ou `test`.
    pub purpose: Option<String>,
    pub cursor: Option<String>,
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LlmCallList {
    pub calls: Vec<LlmCall>,
    pub next_cursor: Option<String>,
}

impl<S: Store> Core<S> {
    pub async fn list_llm_calls(
        &self,
        principal: &Principal,
        query: ListLlmCallsQuery,
    ) -> Result<LlmCallList, ApiError> {
        self.authorize(principal, "list_llm_calls")?;
        let since = match query
            .since
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(since) => Some(ids::timestamp(ids::parse_timestamp(since).ok_or_else(
                || {
                    ApiError::validation("since deve ser um instante RFC 3339")
                        .with_details(json!({ "field": "since" }))
                },
            )?)),
            None => None,
        };
        let page = numeric_page(query.cursor.as_deref(), query.limit)?;
        let filter = LlmCallFilter {
            since,
            purpose: query
                .purpose
                .map(|purpose| purpose.trim().to_owned())
                .filter(|purpose| !purpose.is_empty()),
        };
        let page = self.store().list_llm_calls(filter, page).await?;
        Ok(LlmCallList {
            next_cursor: page.next.as_ref().map(Cursor::encode),
            calls: page.items,
        })
    }

    pub async fn list_jobs(
        &self,
        principal: &Principal,
        query: ListJobsQuery,
    ) -> Result<JobList, ApiError> {
        self.authorize(principal, "list_jobs")?;
        let state = match query
            .state
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(state) => Some(JobState::parse(state).ok_or_else(|| {
                ApiError::validation("state deve ser queued, running, done ou failed")
                    .with_details(json!({ "field": "state" }))
            })?),
            None => None,
        };
        let page = numeric_page(query.cursor.as_deref(), query.limit)?;
        let filter = JobFilter {
            state,
            kind: query
                .kind
                .map(|kind| kind.trim().to_owned())
                .filter(|kind| !kind.is_empty()),
        };
        let page = self.store().list_jobs(filter, page).await?;
        Ok(JobList {
            next_cursor: page.next.as_ref().map(Cursor::encode),
            jobs: page.items,
        })
    }

    pub async fn retry_job(
        &self,
        principal: &Principal,
        job: &str,
        key: Option<&str>,
    ) -> Result<Mutation<Job>, ApiError> {
        let operation = self.authorize(principal, "retry_job")?;
        let id = job_id(job)?;
        let core = self.clone();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &json!({ "job": id }),
                200,
                |context| async move {
                    core.store()
                        .retry_job(id, context)
                        .await
                        .map_err(|error| super::store_error(error, job_not_found))
                },
            )
            .await;
        if result.is_ok() {
            self.jobs_enqueued();
        }
        result
    }
}
