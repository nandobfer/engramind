//! Redator (desenho §7, 006): o job `redact` envia o texto ao LLM como dado e aplica o título
//! (e, com a guarda de sentido, o texto revisado numa revisão `system/redator`). A escrita
//! nunca espera o Redator; ele roda depois do commit, pela fila.

pub mod guard;
pub mod prompt;

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;
use zeroize::Zeroizing;

use super::{
    Core,
    idempotency::Mutation,
    ids,
    jobs::JobOutcome,
    memory_text,
    model::{Actor, Job, NewLlmCall, Redaction, WriteContext},
    principal::Principal,
    settings::{
        self, REDATOR_API_KEY, REDATOR_BASE_URL, REDATOR_ENABLED, REDATOR_FALLBACK_MODELS,
        REDATOR_INSTRUCTIONS, REDATOR_MAX_ATTEMPTS, REDATOR_MIN_SIMILARITY, REDATOR_MODEL,
        REDATOR_PROVIDER, REDATOR_REWRITE_TEXT, REDATOR_TIMEOUT_MS, REDATOR_TITLE_MAX,
        SettingsSnapshot,
    },
};
use crate::{
    error::ApiError,
    providers::llm::{AnyClient, Endpoint, LlmClient, LlmError, LlmRequest, Provider},
    store::{Store, StoreError},
};

pub const JOB_KIND: &str = "redact";
/// Autor das revisões e da auditoria do Redator: `system/redator`.
pub const SYSTEM_APP: &str = "system";
pub const REDATOR_AGENT: &str = "redator";
const SCHEMA_NAME: &str = "redator_result";
const MAX_OUTPUT_TOKENS: u32 = 4096;
const LEASE_MARGIN: Duration = Duration::from_secs(60);
const MAX_ERROR_CHARS: usize = 200;

/// Configuração efetiva do Redator para um canal (ou global, no `test_redator`).
#[derive(Clone)]
pub struct RedatorConfig {
    pub enabled: bool,
    pub provider: Option<Provider>,
    pub model: Option<String>,
    pub api_key: Option<Zeroizing<String>>,
    pub base_url: Option<String>,
    pub instructions: Option<String>,
    pub rewrite_text: bool,
    pub title_max: usize,
    pub min_similarity: f64,
    pub timeout: Duration,
    pub max_attempts: i64,
    pub fallback_models: Vec<String>,
    pub text_min: usize,
    pub text_max: usize,
}

/// Provedor, modelos em ordem e destino prontos para chamar.
pub struct Ready {
    pub provider: Provider,
    pub models: Vec<String>,
    pub client: AnyClient,
}

impl RedatorConfig {
    pub fn new(
        snapshot: &SettingsSnapshot,
        overrides: Option<&BTreeMap<String, Value>>,
        api_key: Option<Zeroizing<String>>,
    ) -> Self {
        let text = |key: &str| {
            snapshot
                .in_channel(key, overrides)
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        Self {
            enabled: snapshot.boolean(REDATOR_ENABLED),
            provider: snapshot
                .optional_text(REDATOR_PROVIDER)
                .as_deref()
                .and_then(Provider::parse),
            model: snapshot.optional_text(REDATOR_MODEL),
            api_key: api_key.filter(|key| !key.trim().is_empty()),
            base_url: snapshot.optional_text(REDATOR_BASE_URL),
            instructions: text(REDATOR_INSTRUCTIONS),
            rewrite_text: snapshot
                .in_channel(REDATOR_REWRITE_TEXT, overrides)
                .as_bool()
                .unwrap_or(true),
            title_max: usize::try_from(snapshot.whole(REDATOR_TITLE_MAX)).unwrap_or(100),
            min_similarity: snapshot.number(REDATOR_MIN_SIMILARITY),
            timeout: Duration::from_millis(
                u64::try_from(snapshot.whole(REDATOR_TIMEOUT_MS)).unwrap_or(30_000),
            ),
            max_attempts: snapshot.whole(REDATOR_MAX_ATTEMPTS).max(1),
            fallback_models: snapshot.text_list(REDATOR_FALLBACK_MODELS),
            text_min: snapshot.text_min(),
            text_max: snapshot.text_max(overrides),
        }
    }

    /// Chaves que faltam para chamar o LLM.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        match self.provider {
            None => missing.push(REDATOR_PROVIDER),
            Some(provider) => {
                if provider.requires_api_key() && self.api_key.is_none() {
                    missing.push(REDATOR_API_KEY);
                }
                if provider.default_base_url().is_none() && self.base_url.is_none() {
                    missing.push(REDATOR_BASE_URL);
                }
            }
        }
        if self.model.is_none() {
            missing.push(REDATOR_MODEL);
        }
        missing
    }

    pub fn ready(&self, http: &reqwest::Client) -> Option<Ready> {
        if !self.missing().is_empty() {
            return None;
        }
        let provider = self.provider?;
        let base_url = self
            .base_url
            .clone()
            .or_else(|| provider.default_base_url().map(str::to_owned))?;
        let mut models = vec![self.model.clone()?];
        for model in &self.fallback_models {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
        Some(Ready {
            provider,
            models,
            client: AnyClient::new(
                provider,
                Endpoint::new(http.clone(), &base_url, self.api_key.clone(), self.timeout),
            ),
        })
    }

    /// Lease do job: cada modelo pode gastar o tempo limite inteiro.
    pub fn lease(&self) -> Duration {
        let models = 1 + u32::try_from(self.fallback_models.len()).unwrap_or(u32::MAX);
        self.timeout.saturating_mul(models) + LEASE_MARGIN
    }
}

/// Resposta validada do Redator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub title: String,
    /// `None` sem revisão do texto.
    pub text: Option<String>,
}

/// Confere a resposta contra o esquema e os limites (FR-006); o motivo é curto e sem texto.
pub fn validate(
    content: &Value,
    rewrite: bool,
    title_max: usize,
    text_min: usize,
    text_max: usize,
) -> Result<Output, &'static str> {
    let object = content.as_object().ok_or("resposta nao e um objeto")?;
    // Sem revisão do texto, um `text` a mais é ignorado; outros campos invalidam.
    if object.keys().any(|key| key != "title" && key != "text") {
        return Err("campo fora do esquema");
    }
    let title = object
        .get("title")
        .and_then(Value::as_str)
        .ok_or("title ausente")?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        return Err("title vazio");
    }
    if title.chars().count() > title_max {
        return Err("title acima de redator.title_max");
    }
    let text = if rewrite {
        let text = object
            .get("text")
            .and_then(Value::as_str)
            .ok_or("text ausente")?
            .trim()
            .to_owned();
        let length = text.chars().count();
        if length < text_min || length > text_max {
            return Err("text fora de memory.text_min/text_max");
        }
        Some(text)
    } else {
        None
    };
    Ok(Output { title, text })
}

/// Backoff da fila: 2 s × 2ⁿ, teto de 10 min (FR-003).
pub fn backoff(attempts: i64) -> Duration {
    let exponent = u32::try_from(attempts.saturating_sub(1).clamp(0, 20)).unwrap_or(20);
    Duration::from_secs(2u64.saturating_mul(2u64.saturating_pow(exponent)))
        .min(Duration::from_secs(600))
}

fn short(error: &str) -> String {
    error.chars().take(MAX_ERROR_CHARS).collect()
}

/// Resultado de uma rodada pelos modelos.
struct Completion {
    result: Result<(Output, String), Failure>,
    calls: Vec<NewLlmCall>,
    duration: Duration,
}

/// Falha da rodada: a do último modelo tentado.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Failure {
    Llm(LlmError),
    Invalid(&'static str),
}

impl Failure {
    fn reason(&self) -> String {
        match self {
            Self::Llm(error) => error.to_string(),
            Self::Invalid(reason) => format!("resposta invalida: {reason}"),
        }
    }
}

/// O texto e os limites de uma chamada.
struct Call<'a> {
    text: &'a str,
    rewrite: bool,
    purpose: &'static str,
    attempt: i64,
}

impl<S: Store> Core<S> {
    async fn redator_config(&self, channel_id: &str) -> Result<RedatorConfig, ApiError> {
        let network = self.network().await?;
        let api_key = self.secret(REDATOR_API_KEY).await?;
        Ok(RedatorConfig::new(
            &self.settings_snapshot(),
            network.channel_settings.get(channel_id),
            api_key,
        ))
    }

    /// Lease do job `redact` com a configuração global.
    pub(crate) fn redact_lease(&self) -> Duration {
        RedatorConfig::new(&self.settings_snapshot(), None, None).lease()
    }

    /// Tenta o modelo principal e os de `fallback_models` (FR-010); cada chamada vira uma
    /// linha de `llm_calls` (FR-011).
    async fn complete(&self, config: &RedatorConfig, ready: &Ready, call: Call<'_>) -> Completion {
        let started = Instant::now();
        let system = prompt::system(
            config.title_max,
            call.rewrite,
            config.instructions.as_deref(),
        );
        let user = prompt::user(call.text, &prompt::boundary());
        let mut calls = Vec::new();
        let mut failure = Failure::Llm(LlmError::Unavailable);
        for (index, model) in ready.models.iter().enumerate() {
            let request = LlmRequest {
                model: model.clone(),
                system: system.clone(),
                user: user.clone(),
                schema_name: SCHEMA_NAME,
                schema: prompt::schema(call.rewrite),
                max_output_tokens: MAX_OUTPUT_TOKENS,
            };
            let at = ids::timestamp(ids::now());
            let began = Instant::now();
            let result = ready.client.complete_json(request).await;
            let duration_ms = i64::try_from(began.elapsed().as_millis()).unwrap_or(i64::MAX);
            let mut record = NewLlmCall {
                at,
                purpose: call.purpose,
                provider: ready.provider.as_str().to_owned(),
                model: model.clone(),
                input_tokens: None,
                output_tokens: None,
                cost_usd: None,
                duration_ms,
                outcome: "success",
                error: None,
            };
            match result {
                Ok(response) => {
                    record.input_tokens = response.input_tokens;
                    record.output_tokens = response.output_tokens;
                    record.cost_usd = response.cost_usd;
                    match validate(
                        &response.content,
                        call.rewrite,
                        config.title_max,
                        config.text_min,
                        config.text_max,
                    ) {
                        Ok(output) => {
                            calls.push(record);
                            return Completion {
                                result: Ok((output, model.clone())),
                                calls,
                                duration: started.elapsed(),
                            };
                        }
                        Err(reason) => {
                            record.outcome = "invalid_response";
                            record.error = Some(short(reason));
                            calls.push(record);
                            failure = Failure::Invalid(reason);
                            break;
                        }
                    }
                }
                Err(error) => {
                    record.outcome = error.outcome();
                    record.error = Some(short(&error.to_string()));
                    calls.push(record);
                    let next = ready.models.get(index + 1);
                    if error.tries_fallback()
                        && let Some(next) = next
                    {
                        tracing::warn!(
                            event = "redator.fallback",
                            provider = ready.provider.as_str(),
                            model = %model,
                            next_model = %next,
                            outcome = error.outcome(),
                            attempt = call.attempt,
                            duration_ms,
                            "modelo do Redator falhou; tentando o proximo de fallback_models"
                        );
                        failure = Failure::Llm(error);
                        continue;
                    }
                    failure = Failure::Llm(error);
                    break;
                }
            }
        }
        Completion {
            result: Err(failure),
            calls,
            duration: started.elapsed(),
        }
    }

    async fn record_calls(&self, calls: Vec<NewLlmCall>) {
        if calls.is_empty() {
            return;
        }
        if let Err(error) = self.store().append_llm_calls(calls, None).await {
            tracing::warn!(event = "llm_calls.write_failed", error = %error, "falha ao gravar llm_calls");
        }
    }

    async fn fallback_title(&self, memory_id: &str, hash: Vec<u8>) {
        if let Err(error) = self
            .store()
            .set_title_fallback(memory_id.to_owned(), hash)
            .await
        {
            tracing::warn!(event = "redator.fallback_failed", memory_id, error = %error, "falha ao marcar title_state = fallback");
        }
    }

    /// Executa um job `redact`.
    pub(crate) async fn run_redact(&self, job: &Job) -> JobOutcome {
        let Some(memory_id) = job.payload["memory_id"].as_str().map(str::to_owned) else {
            return JobOutcome::Fail("payload sem memory_id".to_owned());
        };
        let title_only = job.payload["title_only"].as_bool().unwrap_or(false);
        let memory = match self.store().get_memory(memory_id.clone()).await {
            Ok(Some(memory)) => memory,
            Ok(None) => return JobOutcome::Superseded,
            Err(error) => return JobOutcome::Retry(short(&error.to_string())),
        };
        match self
            .store()
            .newer_job_exists(JOB_KIND.to_owned(), memory_id.clone(), job.id)
            .await
        {
            Ok(true) => return JobOutcome::Superseded,
            Ok(false) => {}
            Err(error) => return JobOutcome::Retry(short(&error.to_string())),
        }
        let hash = memory_text::content_hash(&memory.text);
        let config = match self.redator_config(&memory.channel_id).await {
            Ok(config) => config,
            Err(error) => return JobOutcome::Retry(short(&error.message)),
        };
        let ready = if config.enabled {
            config.ready(self.http())
        } else {
            None
        };
        let Some(ready) = ready else {
            // FR-009: desligado ou sem configuração → fallback, sem chamar nada e sem ERROR.
            self.fallback_title(&memory_id, hash).await;
            tracing::debug!(
                event = "redator.skipped",
                memory_id = %memory_id,
                enabled = config.enabled,
                missing = ?config.missing(),
                "Redator desligado ou sem configuracao; titulo provisorio mantido"
            );
            return JobOutcome::Done("skipped");
        };
        let rewrite = config.rewrite_text && !title_only;
        let completion = self
            .complete(
                &config,
                &ready,
                Call {
                    text: &memory.text,
                    rewrite,
                    purpose: JOB_KIND,
                    attempt: job.attempts,
                },
            )
            .await;
        self.record_calls(completion.calls).await;
        let duration_ms = i64::try_from(completion.duration.as_millis()).unwrap_or(i64::MAX);
        let (output, model) = match completion.result {
            Ok(result) => result,
            Err(failure) => {
                let reason = failure.reason();
                if job.attempts >= config.max_attempts {
                    self.fallback_title(&memory_id, hash).await;
                    return JobOutcome::Fail(short(&reason));
                }
                tracing::warn!(
                    event = "redator.retry",
                    memory_id = %memory_id,
                    provider = ready.provider.as_str(),
                    attempt = job.attempts,
                    max_attempts = config.max_attempts,
                    error = %reason,
                    duration_ms,
                    "Redator falhou; job reagendado com backoff"
                );
                return JobOutcome::Retry(short(&reason));
            }
        };
        let text = self
            .revised_text(
                &memory.text,
                &memory.channel_id,
                &memory_id,
                output.text,
                &config,
            )
            .await;
        let applied = if text.is_some() {
            "applied"
        } else {
            "title_only"
        };
        let redaction = Redaction {
            memory_id: memory_id.clone(),
            expected_hash: hash,
            title: output.title,
            text: text.map(|text| {
                let hash = memory_text::content_hash(&text);
                (text, hash)
            }),
            embed: self.embeddings_enabled(),
        };
        let context = WriteContext {
            actor: Actor {
                app_id: Some(SYSTEM_APP.to_owned()),
                agent_id: Some(REDATOR_AGENT.to_owned()),
                token_id: None,
            },
            at: ids::timestamp(ids::now()),
            idempotency: None,
        };
        match self.store().apply_redaction(redaction, context).await {
            Ok(_) => {
                self.network_changed();
                self.jobs_enqueued();
                tracing::info!(
                    event = "redator.completed",
                    memory_id = %memory_id,
                    provider = ready.provider.as_str(),
                    model = %model,
                    outcome = applied,
                    attempt = job.attempts,
                    duration_ms,
                    "Redator aplicado"
                );
                JobOutcome::Done(applied)
            }
            Err(StoreError::NotFound | StoreError::Conflict("MEMORY_CHANGED")) => {
                JobOutcome::Superseded
            }
            Err(error) => JobOutcome::Retry(short(&error.to_string())),
        }
    }

    /// O texto revisado só passa se mudou, cabe no canal sem duplicar outra memória e a guarda
    /// de sentido confirma (Q-001, Q-005); senão, só o título.
    async fn revised_text(
        &self,
        original: &str,
        channel_id: &str,
        memory_id: &str,
        revised: Option<String>,
        config: &RedatorConfig,
    ) -> Option<String> {
        let revised = revised?;
        if memory_text::content_hash(&revised) == memory_text::content_hash(original) {
            return None;
        }
        match self
            .store()
            .find_memory_by_hash(
                channel_id.to_owned(),
                memory_text::content_hash(&revised),
                Some(memory_id.to_owned()),
            )
            .await
        {
            Ok(None) => {}
            Ok(Some(_)) | Err(_) => return None,
        }
        let similarity = match self.meaning_guard().await {
            Some(guard) => guard.similarity(original, &revised).await,
            None => None,
        };
        match similarity {
            Some(similarity) if similarity >= config.min_similarity => Some(revised),
            Some(similarity) => {
                tracing::debug!(
                    event = "redator.guard_rejected",
                    memory_id,
                    similarity,
                    min_similarity = config.min_similarity,
                    "texto revisado mudou o sentido; mantido o original"
                );
                None
            }
            None => {
                tracing::debug!(
                    event = "redator.guard_unavailable",
                    memory_id,
                    "sem guarda de sentido; mantido o texto original"
                );
                None
            }
        }
    }
}

// ---------- test_redator ----------

/// Corpo de `test_redator`: um texto de exemplo e, opcionalmente, uma configuração proposta
/// (chaves `redator.*`, inclusive `redator.api_key` em claro), que não é gravada.
#[derive(Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestRedatorBody {
    /// Texto de exemplo, com os limites de `memory.text_min`/`text_max`.
    #[schema(example = "Use cargo nextest para rodar os testes do servico localmente.")]
    pub text: String,
    /// Mapa parcial `redator.* → valor` usado só neste teste.
    #[serde(default)]
    #[schema(value_type = Option<Object>, example = json!({ "redator.model": "gpt-4o-mini" }))]
    pub settings: Option<BTreeMap<String, Value>>,
}

impl std::fmt::Debug for TestRedatorBody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TestRedatorBody([REDACTED])")
    }
}

/// Resultado do teste: o que o Redator faria com o texto (nada é gravado além de `llm_calls`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TestRedatorResult {
    #[schema(example = "Testes locais com cargo nextest")]
    pub title: String,
    /// Texto revisado (o original com `redator.rewrite_text = false`).
    pub text: String,
    /// Modelo que respondeu (o principal ou um de `fallback_models`).
    #[schema(example = "gpt-4o-mini")]
    pub model: String,
    #[schema(example = 850)]
    pub latency_ms: i64,
}

pub(crate) fn provider_not_configured(missing: &[&str]) -> ApiError {
    ApiError::new(
        "PROVIDER_NOT_CONFIGURED",
        "Redator sem provedor, modelo, URL ou chave configurados",
    )
    .with_details(json!({ "missing": missing }))
}

fn provider_error(failure: &Failure) -> ApiError {
    match failure {
        Failure::Llm(LlmError::Timeout) => ApiError::new(
            "PROVIDER_TIMEOUT",
            "O provedor do Redator nao respondeu dentro de redator.timeout_ms",
        )
        .with_details(json!({ "outcome": "timeout" })),
        Failure::Llm(error) => ApiError::new(
            "PROVIDER_FAILED",
            format!("O provedor do Redator falhou: {error}"),
        )
        .with_details(json!({ "outcome": error.outcome(), "reason": short(&error.to_string()) })),
        Failure::Invalid(reason) => ApiError::new(
            "PROVIDER_FAILED",
            format!("O provedor do Redator devolveu uma resposta invalida: {reason}"),
        )
        .with_details(json!({ "outcome": "invalid_response", "reason": reason })),
    }
}

impl<S: Store> Core<S> {
    pub async fn test_redator(
        &self,
        principal: &Principal,
        body: TestRedatorBody,
        key: Option<&str>,
    ) -> Result<Mutation<TestRedatorResult>, ApiError> {
        let operation = self.authorize(principal, "test_redator")?;
        let snapshot = self.settings_snapshot();
        let text =
            super::memories::memory_text(&body.text, snapshot.text_min(), snapshot.text_max(None))?;
        let mut proposed = BTreeMap::new();
        let mut proposed_key = None;
        for (name, value) in body.settings.iter().flatten() {
            let spec = settings::spec(name)
                .filter(|spec| spec.key.starts_with("redator."))
                .ok_or_else(|| {
                    ApiError::new(
                        "SETTING_UNKNOWN",
                        format!("Configuracao desconhecida para o Redator: {name}"),
                    )
                    .with_details(json!({
                        "key": name,
                        "valid": settings::SETTINGS
                            .iter()
                            .filter(|spec| spec.key.starts_with("redator."))
                            .map(|spec| spec.key)
                            .collect::<Vec<_>>(),
                    }))
                })?;
            if spec.key == REDATOR_API_KEY {
                match value {
                    Value::String(secret) if !secret.trim().is_empty() => {
                        proposed_key = Some(Zeroizing::new(secret.trim().to_owned()));
                    }
                    _ => {
                        return Err(ApiError::new(
                            "SETTING_INVALID",
                            "redator.api_key proposta deve ser um texto",
                        )
                        .with_details(json!({ "key": REDATOR_API_KEY, "type": "secret" })));
                    }
                }
                continue;
            }
            settings::validate(spec, value)?;
            proposed.insert(spec.key, value.clone());
        }
        let api_key = match proposed_key {
            Some(key) => Some(key),
            None => self.secret(REDATOR_API_KEY).await?,
        };
        let config = RedatorConfig::new(&snapshot.with(&proposed), None, api_key);
        let Some(ready) = config.ready(self.http()) else {
            return Err(provider_not_configured(&config.missing()));
        };
        let core = self.clone();
        self.mutate(
            principal,
            operation,
            key,
            &body,
            200,
            |context| async move {
                let completion = core
                    .complete(
                        &config,
                        &ready,
                        Call {
                            text: &text,
                            rewrite: config.rewrite_text,
                            purpose: "test",
                            attempt: 1,
                        },
                    )
                    .await;
                let latency_ms = i64::try_from(completion.duration.as_millis()).unwrap_or(i64::MAX);
                match completion.result {
                    Ok((output, model)) => {
                        let result = TestRedatorResult {
                            title: output.title,
                            text: output.text.unwrap_or_else(|| text.clone()),
                            model,
                            latency_ms,
                        };
                        let response =
                            serde_json::to_value(&result).map_err(|_| ApiError::internal())?;
                        core.store()
                            .append_llm_calls(completion.calls, Some((context, response)))
                            .await
                            .map_err(|error| super::store_error(error, ApiError::internal))?;
                        Ok(result)
                    }
                    Err(failure) => {
                        core.record_calls(completion.calls).await;
                        Err(provider_error(&failure))
                    }
                }
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn responses_are_validated_against_schema_and_limits() {
        let ok = validate(
            &json!({ "title": "  Titulo \n curto ", "text": " Texto revisado " }),
            true,
            20,
            3,
            100,
        )
        .expect("valid");
        assert_eq!(ok.title, "Titulo curto");
        assert_eq!(ok.text.as_deref(), Some("Texto revisado"));
        let title_only = validate(
            &json!({ "title": "T", "text": "ignorado" }),
            false,
            20,
            3,
            100,
        )
        .expect("title only");
        assert_eq!(title_only.text, None);
        for (content, rewrite) in [
            (json!("texto"), true),
            (json!({ "text": "abc" }), true),
            (json!({ "title": "", "text": "abc" }), true),
            (json!({ "title": "x".repeat(21), "text": "abc" }), true),
            (json!({ "title": "T" }), true),
            (json!({ "title": "T", "text": "ab" }), true),
            (json!({ "title": "T", "text": "x".repeat(101) }), true),
            (json!({ "title": "T", "text": "abc", "extra": 1 }), true),
            (json!({ "title": "T", "extra": 1 }), false),
            (json!({ "title": 1 }), false),
        ] {
            assert!(
                validate(&content, rewrite, 20, 3, 100).is_err(),
                "{content}"
            );
        }
    }

    #[test]
    fn backoff_doubles_from_two_seconds_up_to_ten_minutes() {
        assert_eq!(backoff(1), Duration::from_secs(2));
        assert_eq!(backoff(2), Duration::from_secs(4));
        assert_eq!(backoff(3), Duration::from_secs(8));
        assert_eq!(backoff(9), Duration::from_secs(512));
        assert_eq!(backoff(10), Duration::from_secs(600));
        assert_eq!(backoff(50), Duration::from_secs(600));
        assert_eq!(backoff(0), Duration::from_secs(2));
    }

    #[test]
    fn configuration_reports_what_is_missing() {
        let snapshot = SettingsSnapshot::default();
        let config = RedatorConfig::new(&snapshot, None, None);
        assert_eq!(config.missing(), vec![REDATOR_PROVIDER, REDATOR_MODEL]);
        assert!(config.ready(&reqwest::Client::new()).is_none());
        let proposed = BTreeMap::from([
            (REDATOR_PROVIDER, json!("openai_compatible")),
            (REDATOR_MODEL, json!("m")),
        ]);
        let openai = RedatorConfig::new(&snapshot.with(&proposed), None, None);
        assert_eq!(openai.missing(), vec![REDATOR_BASE_URL]);
        let proposed = BTreeMap::from([
            (REDATOR_PROVIDER, json!("anthropic")),
            (REDATOR_MODEL, json!("m")),
            (REDATOR_FALLBACK_MODELS, json!(["m", "n"])),
        ]);
        let anthropic = RedatorConfig::new(&snapshot.with(&proposed), None, None);
        assert_eq!(anthropic.missing(), vec![REDATOR_API_KEY]);
        let keyed = RedatorConfig::new(
            &snapshot.with(&proposed),
            None,
            Some(Zeroizing::new("k".to_owned())),
        );
        let ready = keyed.ready(&reqwest::Client::new()).expect("ready");
        assert_eq!(ready.models, vec!["m".to_owned(), "n".to_owned()]);
        assert_eq!(
            keyed.lease(),
            Duration::from_millis(30_000 * 3) + Duration::from_secs(60)
        );
    }
}
