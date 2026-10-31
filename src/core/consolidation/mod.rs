//! Consolidação (desenho §10, 009): duplicatas fundidas pelo Redator, importância para a ordem
//! de corte do contexto e desuso. Um job `consolidate` por canal, agendado por cron
//! (`consolidation.schedule`) ou pedido com `run_consolidation`; o `dry_run` devolve o plano sem
//! gravar nem chamar o LLM.

pub mod job;
pub mod plan;
pub mod schedule;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use self::plan::{ChannelPlan, PlanOptions, SkipReason};
use super::{
    Core,
    idempotency::Mutation,
    ids,
    model::{Actor, Channel, WriteContext},
    principal::Principal,
    redator::SYSTEM_APP,
    settings::{
        CONSOLIDATION_AUTO_ARCHIVE_STALE, CONSOLIDATION_DUPLICATE_THRESHOLD, CONSOLIDATION_ENABLED,
        CONSOLIDATION_SCHEDULE, CONSOLIDATION_STALE_AFTER_DAYS, CONSOLIDATION_TASKS,
        SettingsSnapshot,
    },
};
use crate::{error::ApiError, store::Store};

pub const JOB_KIND: &str = "consolidate";
/// Autor das revisões e da auditoria: `system/consolidador` (D-003).
pub const CONSOLIDATOR_AGENT: &str = "consolidador";
/// Finalidade das chamadas em `llm_calls` (FR-007).
pub const PURPOSE: &str = "consolidation";
/// Resumo da última execução por canal, para o `get_status`.
pub const META_LAST_RUN: &str = "consolidation.last_run";

/// Tarefa do consolidador; a ordem das variantes é a de execução.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    ToSchema,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    /// Pares parecidos que o Redator funde.
    Duplicates,
    /// Nota de 0 a 1 para a ordem de corte do contexto.
    Importance,
    /// Sinaliza (ou arquiva) as memórias sem uso.
    Stale,
}

impl Task {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Duplicates => "duplicates",
            Self::Importance => "importance",
            Self::Stale => "stale",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "duplicates" => Some(Self::Duplicates),
            "importance" => Some(Self::Importance),
            "stale" => Some(Self::Stale),
            _ => None,
        }
    }
}

/// Uma memória `active` do canal, como o planejador e a execução precisam.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidationMemory {
    pub id: String,
    pub category: String,
    pub title: String,
    pub text: String,
    pub content_hash: Vec<u8>,
    pub created_at: String,
    pub updated_at: String,
    pub importance: Option<f64>,
    pub importance_pending: Option<f64>,
    pub last_retrieved_at: Option<String>,
    pub last_injected_at: Option<String>,
    pub verified_at: Option<String>,
    pub stale_candidate: bool,
    /// Vetor do modelo vigente, quando existe.
    pub vector: Option<Vec<f32>>,
}

/// Par que o Redator recusou fundir (Q-014), com `memory_a < memory_b`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclinedPair {
    pub memory_a: String,
    pub memory_b: String,
    pub hash_a: Vec<u8>,
    pub hash_b: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConsolidationInput {
    pub memories: Vec<ConsolidationMemory>,
    pub declined: Vec<DeclinedPair>,
}

/// Configuração efetiva de um canal.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidationConfig {
    pub enabled: bool,
    pub tasks: Vec<Task>,
    pub duplicate_threshold: f64,
    pub stale_after_days: i64,
    pub auto_archive_stale: bool,
}

impl ConsolidationConfig {
    pub fn new(snapshot: &SettingsSnapshot, overrides: Option<&BTreeMap<String, Value>>) -> Self {
        let mut tasks: Vec<Task> = snapshot
            .text_list(CONSOLIDATION_TASKS)
            .iter()
            .filter_map(|task| Task::parse(task))
            .collect();
        tasks.sort();
        tasks.dedup();
        Self {
            enabled: snapshot
                .in_channel(CONSOLIDATION_ENABLED, overrides)
                .as_bool()
                .unwrap_or(false),
            tasks,
            duplicate_threshold: snapshot.number(CONSOLIDATION_DUPLICATE_THRESHOLD),
            stale_after_days: snapshot.whole(CONSOLIDATION_STALE_AFTER_DAYS).max(1),
            auto_archive_stale: snapshot.boolean(CONSOLIDATION_AUTO_ARCHIVE_STALE),
        }
    }
}

// ---------- contrato ----------

/// Corpo de `run_consolidation`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunConsolidationBody {
    /// Canal (id `ch_…` ou slug); ausente: todos os canais com `consolidation.enabled`.
    #[schema(example = "eng/global")]
    #[serde(default)]
    pub channel: Option<String>,
    /// Tarefas (padrão `consolidation.tasks`); executadas sempre na ordem `duplicates`,
    /// `importance`, `stale`.
    #[serde(default)]
    pub tasks: Option<Vec<Task>>,
    /// `true`: devolve o plano sem gravar e sem chamar o LLM.
    #[serde(default)]
    pub dry_run: Option<bool>,
}

/// Par candidato do plano.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlannedMerge {
    /// A mais antiga: recebe o texto fundido.
    #[schema(example = "m_1b7d0e44")]
    pub keep: String,
    /// Vira `archived` com `merged_into = keep`.
    #[schema(example = "m_8f2a1c3d")]
    pub archive: String,
    /// Cosseno entre os vetores.
    #[schema(example = 0.95)]
    pub similarity: f64,
}

/// Desuso previsto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PlannedStale {
    /// Ganham `stale_candidate = true`.
    pub flag: Vec<String>,
    /// Voltaram a ser usadas: `stale_candidate = false`.
    pub clear: Vec<String>,
    /// Arquivadas (`consolidation.auto_archive_stale`).
    pub archive: Vec<String>,
}

/// Plano de um canal: os candidatos que a execução avalia.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ConsolidationPlan {
    #[schema(example = "eng/global")]
    pub channel: String,
    /// Tarefa pulada → motivo (`redator_not_configured`, `embeddings_off`).
    #[schema(value_type = Object, example = json!({ "importance": "redator_not_configured" }))]
    pub skipped: BTreeMap<String, SkipReason>,
    pub duplicates: Vec<PlannedMerge>,
    /// Memórias que recebem nota de importância.
    pub classify: Vec<String>,
    pub stale: PlannedStale,
}

/// Job enfileirado por canal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ConsolidationJobRef {
    #[schema(example = "eng/global")]
    pub channel: String,
    #[schema(example = 481)]
    pub job_id: i64,
    /// O canal já tinha um `consolidate` na fila ou rodando; devolve esse.
    pub already_queued: bool,
}

/// Resposta de `run_consolidation`: o plano (`dry_run`) ou os jobs enfileirados.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ConsolidationRun {
    pub dry_run: bool,
    /// Planos por canal (só com `dry_run`).
    pub channels: Vec<ConsolidationPlan>,
    /// Jobs `consolidate` (só sem `dry_run`).
    pub jobs: Vec<ConsolidationJobRef>,
}

/// Pedido de enfileiramento ao `Store`: um job por canal sem `consolidate` pendente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidationEnqueue {
    /// `(id, slug)`.
    pub channels: Vec<(String, String)>,
    pub tasks: Vec<Task>,
    /// `schedule` ou `manual`.
    pub trigger: &'static str,
    /// Agendado: horário a gravar em `schema_meta['consolidation.last_scheduled']`.
    pub scheduled_at: Option<String>,
}

/// Fila `consolidate` e as contagens somadas das execuções recentes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConsolidationActivity {
    pub queued: u64,
    pub running: u64,
    /// `ConsolidationReport` somado (chaves `merged`, `declined`…).
    pub totals: Value,
}

/// Ações somadas das execuções das últimas 24 h.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ConsolidationTotals {
    pub merged: u64,
    pub declined: u64,
    pub classified: u64,
    pub flagged: u64,
    pub cleared: u64,
    pub archived: u64,
    pub superseded: u64,
    pub failed: u64,
}

/// `get_status.consolidation` (FR-012).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ConsolidationStatus {
    /// Canais ativos com `consolidation.enabled` efetivo.
    #[schema(example = 3)]
    pub enabled_channels: u64,
    #[schema(example = "0 3 * * *")]
    pub schedule: String,
    /// Próximo horário (UTC); `null` com a expressão inválida.
    pub next_run_at: Option<String>,
    /// Último horário que o agendador disparou.
    pub last_scheduled_at: Option<String>,
    /// Jobs `consolidate` na fila.
    pub queued: u64,
    pub running: u64,
    pub last_24h: ConsolidationTotals,
    /// Chamadas ao LLM com `purpose = consolidation` nas últimas 24 h.
    pub llm_last_24h: super::model::LlmSummary,
}

pub(crate) fn consolidation_disabled(slug: &str) -> ApiError {
    ApiError::new(
        "CONSOLIDATION_DISABLED",
        "A consolidacao esta desligada nesse canal (consolidation.enabled)",
    )
    .with_details(json!({ "channel": slug }))
}

pub(crate) fn system_actor() -> Actor {
    Actor {
        app_id: Some(SYSTEM_APP.to_owned()),
        agent_id: Some(CONSOLIDATOR_AGENT.to_owned()),
        token_id: None,
    }
}

pub(crate) fn system_context(events: Vec<super::events::EventDraft>) -> WriteContext {
    WriteContext {
        actor: system_actor(),
        at: ids::timestamp(ids::now()),
        idempotency: None,
        events,
    }
}

/// Plano público a partir do plano interno.
pub fn view(slug: &str, input: &ConsolidationInput, plan: &ChannelPlan) -> ConsolidationPlan {
    let id = |index: &usize| input.memories[*index].id.clone();
    ConsolidationPlan {
        channel: slug.to_owned(),
        skipped: plan
            .skipped
            .iter()
            .map(|(task, reason)| (task.as_str().to_owned(), *reason))
            .collect(),
        duplicates: plan
            .duplicates
            .iter()
            .map(|pair| PlannedMerge {
                keep: id(&pair.keep),
                archive: id(&pair.archive),
                similarity: pair.similarity,
            })
            .collect(),
        classify: plan.classify.iter().flatten().map(id).collect(),
        stale: PlannedStale {
            flag: plan.stale.flag.iter().map(id).collect(),
            clear: plan.stale.clear.iter().map(id).collect(),
            archive: plan.stale.archive.iter().map(id).collect(),
        },
    }
}

fn tasks(value: Option<Vec<Task>>, default: &[Task]) -> Result<Vec<Task>, ApiError> {
    let mut tasks = value.unwrap_or_else(|| default.to_vec());
    if tasks.is_empty() {
        return Err(ApiError::validation(
            "tasks precisa de ao menos uma tarefa (duplicates, importance, stale)",
        ));
    }
    tasks.sort();
    tasks.dedup();
    Ok(tasks)
}

impl<S: Store> Core<S> {
    /// Configuração efetiva do canal.
    pub(crate) fn consolidation_config_for(
        &self,
        overrides: Option<&BTreeMap<String, Value>>,
    ) -> ConsolidationConfig {
        ConsolidationConfig::new(&self.settings_snapshot(), overrides)
    }

    /// Lê o canal e monta o plano das tarefas pedidas.
    pub(crate) async fn channel_plan(
        &self,
        channel_id: &str,
        tasks: &[Task],
        now: DateTime<Utc>,
    ) -> Result<(ConsolidationInput, ChannelPlan), ApiError> {
        let network = self.network().await?;
        let config = self.consolidation_config_for(network.channel_settings.get(channel_id));
        let redator = self.redator_config(channel_id).await?;
        let redator_ready = redator.enabled && redator.ready(self.http()).is_some();
        let input = self
            .store()
            .consolidation_input(channel_id.to_owned())
            .await?;
        let stale_before = ids::timestamp(now - chrono::Duration::days(config.stale_after_days));
        let options = PlanOptions {
            tasks: tasks.to_vec(),
            duplicate_threshold: config.duplicate_threshold,
            stale_before,
            auto_archive_stale: config.auto_archive_stale,
            redator_ready,
            embeddings_on: self.embeddings_enabled(),
        };
        let plan = plan::plan(&input.memories, &input.declined, &options);
        Ok((input, plan))
    }

    /// `run_consolidation` (FR-005, Q-011).
    pub async fn run_consolidation(
        &self,
        principal: &Principal,
        body: RunConsolidationBody,
        key: Option<&str>,
    ) -> Result<Mutation<ConsolidationRun>, ApiError> {
        let operation = self.authorize(principal, "run_consolidation")?;
        let dry_run = body.dry_run.unwrap_or(false);
        let network = self.network().await?;
        let default_tasks = self.consolidation_config_for(None).tasks;
        let tasks = tasks(body.tasks.clone(), &default_tasks)?;
        let channels: Vec<&Channel> = match body.channel.as_deref().map(str::trim) {
            Some(wanted) => {
                let channel = network
                    .channels
                    .iter()
                    .find(|channel| channel.id == wanted || channel.slug == wanted)
                    .ok_or_else(super::channels::channel_not_found)?;
                if channel.archived_at.is_some() {
                    return Err(ApiError::new(
                        "CHANNEL_ARCHIVED",
                        "O canal esta arquivado e nao e consolidado",
                    ));
                }
                let enabled = self
                    .consolidation_config_for(network.channel_settings.get(&channel.id))
                    .enabled;
                if !enabled && !dry_run {
                    return Err(consolidation_disabled(&channel.slug));
                }
                vec![channel]
            }
            None => network
                .channels
                .iter()
                .filter(|channel| channel.archived_at.is_none())
                .filter(|channel| {
                    self.consolidation_config_for(network.channel_settings.get(&channel.id))
                        .enabled
                })
                .collect(),
        };
        let mut channels: Vec<(String, String)> = channels
            .into_iter()
            .map(|channel| (channel.id.clone(), channel.slug.clone()))
            .collect();
        channels.sort_by(|a, b| a.1.cmp(&b.1));
        if dry_run {
            let now = ids::now();
            let mut plans = Vec::new();
            for (id, slug) in &channels {
                let (input, plan) = self.channel_plan(id, &tasks, now).await?;
                plans.push(view(slug, &input, &plan));
            }
            return Ok(Mutation {
                value: ConsolidationRun {
                    dry_run: true,
                    channels: plans,
                    jobs: Vec::new(),
                },
                replayed: false,
            });
        }
        let input = json!({ "body": body });
        let core = self.clone();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                202,
                |context| async move {
                    core.store()
                        .enqueue_consolidation(
                            ConsolidationEnqueue {
                                channels,
                                tasks,
                                trigger: "manual",
                                scheduled_at: None,
                            },
                            context,
                        )
                        .await
                        .map_err(ApiError::from)
                },
            )
            .await;
        self.jobs_enqueued();
        result
    }

    /// `get_status.consolidation`.
    pub(crate) async fn consolidation_status(
        &self,
        since: String,
    ) -> Result<ConsolidationStatus, ApiError> {
        let network = self.network().await?;
        let snapshot = self.settings_snapshot();
        let schedule = snapshot
            .optional_text(CONSOLIDATION_SCHEDULE)
            .unwrap_or_default();
        let enabled_channels = network
            .channels
            .iter()
            .filter(|channel| channel.archived_at.is_none())
            .filter(|channel| {
                self.consolidation_config_for(network.channel_settings.get(&channel.id))
                    .enabled
            })
            .count() as u64;
        let last_scheduled_at = self
            .store()
            .meta(schedule::META_LAST_SCHEDULED)
            .await?
            .and_then(|bytes| String::from_utf8(bytes).ok());
        let base = last_scheduled_at
            .as_deref()
            .and_then(ids::parse_timestamp)
            .unwrap_or_else(ids::now);
        let next_run_at = schedule::next_after(&schedule, base.max(ids::now()))
            .or_else(|| schedule::next_after(&schedule, base))
            .map(ids::timestamp);
        let activity = self.store().consolidation_activity(since.clone()).await?;
        let last_24h = serde_json::from_value(activity.totals).unwrap_or_default();
        Ok(ConsolidationStatus {
            enabled_channels,
            schedule,
            next_run_at,
            last_scheduled_at,
            queued: activity.queued,
            running: activity.running,
            last_24h,
            llm_last_24h: self.store().llm_summary(PURPOSE.to_owned(), since).await?,
        })
    }

    /// Agendador (FR-008): chamado a cada minuto; enfileira um `consolidate` por canal
    /// habilitado quando passou um horário de `consolidation.schedule`. Devolve quantos jobs
    /// novos.
    pub async fn consolidation_tick(&self, now: DateTime<Utc>) -> Result<usize, ApiError> {
        let snapshot = self.settings_snapshot();
        let expression = snapshot
            .optional_text(CONSOLIDATION_SCHEDULE)
            .unwrap_or_default();
        let last = self
            .store()
            .meta(schedule::META_LAST_SCHEDULED)
            .await?
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| ids::parse_timestamp(&text));
        let Some(last) = last else {
            // Primeiro start: só registra a base, sem disparar (Q-010).
            self.store()
                .set_meta(
                    schedule::META_LAST_SCHEDULED,
                    ids::timestamp(now).as_bytes(),
                )
                .await?;
            return Ok(0);
        };
        if !schedule::due(&expression, last, now) {
            return Ok(0);
        }
        let network = self.network().await?;
        let tasks = self.consolidation_config_for(None).tasks;
        let mut channels: Vec<(String, String)> = network
            .channels
            .iter()
            .filter(|channel| channel.archived_at.is_none())
            .filter(|channel| {
                self.consolidation_config_for(network.channel_settings.get(&channel.id))
                    .enabled
            })
            .map(|channel| (channel.id.clone(), channel.slug.clone()))
            .collect();
        channels.sort_by(|a, b| a.1.cmp(&b.1));
        if tasks.is_empty() {
            channels.clear();
        }
        let run = self
            .store()
            .enqueue_consolidation(
                ConsolidationEnqueue {
                    channels,
                    tasks,
                    trigger: "schedule",
                    scheduled_at: Some(ids::timestamp(now)),
                },
                system_context(Vec::new()),
            )
            .await?;
        let created = run.jobs.iter().filter(|job| !job.already_queued).count();
        if !run.jobs.is_empty() {
            tracing::info!(
                event = "consolidation.scheduled",
                channels = run.jobs.len(),
                created,
                "consolidacao agendada enfileirada"
            );
        }
        self.jobs_enqueued();
        Ok(created)
    }
}
