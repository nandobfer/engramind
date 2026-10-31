//! Embeddings (desenho §11, 007): configuração efetiva, o provedor pronto para chamar, a troca
//! de modelo com reindexação, `test_embedding`, `list_embedding_models`, `reembed_all` e o
//! bloco `embedding` de `get_status`. O job `embed` está em `job.rs`.

pub mod job;

use std::{collections::BTreeMap, sync::Arc, time::Duration, time::Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;
use zeroize::Zeroizing;

use super::{
    Core,
    idempotency::Mutation,
    ids,
    model::{EmbeddingSwitch, ReindexPlan, ReindexState},
    principal::Principal,
    redator::guard::{GuardFuture, MeaningGuard},
    settings::{
        self, EMBEDDING_API_KEY, EMBEDDING_BASE_URL, EMBEDDING_BATCH_SIZE, EMBEDDING_DIMENSIONS,
        EMBEDDING_DOCUMENT_PREFIX, EMBEDDING_IDENTITY_KEYS, EMBEDDING_MODEL, EMBEDDING_PROVIDER,
        EMBEDDING_QUERY_PREFIX, EMBEDDING_TIMEOUT_MS, SettingsSnapshot,
    },
};
use crate::{
    error::ApiError,
    providers::embeddings::{
        EmbedError, Embedder, EmbeddingModelEntry, EmbeddingProvider, Endpoint, Gemini, LoadState,
        Ollama, OpenAiCompatible, list_remote_models, local,
    },
    store::{Store, StoreError},
};

pub const JOB_KIND: &str = "embed";
/// Tentativas do job `embed` (007 Q-011); esgotadas, `embedding_state = failed`.
pub const MAX_ATTEMPTS: i64 = 5;
/// Espera máxima pela consulta em `search_memories` e `get_hints` (007 Q-019): passado o
/// prazo, a busca segue só com o FTS.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const LEASE_MARGIN: Duration = Duration::from_secs(60);
/// Texto da validação feita ao salvar uma troca de provedor ou modelo.
const SAMPLE: &str = "amostra";
const MAX_ERROR_CHARS: usize = 200;

/// O que vetorizar: a consulta ou a memória (cada uma com o seu prefixo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedKind {
    Query,
    Document,
}

/// Prefixos automáticos: a família e5 espera `query: ` e `passage: ` (desenho §11).
fn auto_prefixes(model: &str) -> (&'static str, &'static str) {
    if model.to_ascii_lowercase().contains("e5-") {
        ("query: ", "passage: ")
    } else {
        ("", "")
    }
}

/// Configuração efetiva de embedding (global; não há sobreposição por canal).
#[derive(Clone)]
pub struct EmbeddingConfig {
    /// `None` com `embedding.provider = off`.
    pub provider: Option<EmbeddingProvider>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<Zeroizing<String>>,
    pub dimensions: Option<usize>,
    pub query_prefix: Option<String>,
    pub document_prefix: Option<String>,
    pub batch_size: usize,
    pub timeout: Duration,
}

impl EmbeddingConfig {
    pub fn new(snapshot: &SettingsSnapshot, api_key: Option<Zeroizing<String>>) -> Self {
        let raw = |key: &str| {
            snapshot
                .value(key)
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .map(str::to_owned)
        };
        Self {
            provider: snapshot
                .optional_text(EMBEDDING_PROVIDER)
                .as_deref()
                .and_then(EmbeddingProvider::parse),
            model: snapshot.optional_text(EMBEDDING_MODEL),
            base_url: snapshot.optional_text(EMBEDDING_BASE_URL),
            api_key: api_key.filter(|key| !key.trim().is_empty()),
            dimensions: snapshot
                .value(EMBEDDING_DIMENSIONS)
                .as_u64()
                .and_then(|dims| usize::try_from(dims).ok()),
            query_prefix: raw(EMBEDDING_QUERY_PREFIX),
            document_prefix: raw(EMBEDDING_DOCUMENT_PREFIX),
            batch_size: usize::try_from(snapshot.whole(EMBEDDING_BATCH_SIZE))
                .unwrap_or(32)
                .max(1),
            timeout: Duration::from_millis(
                u64::try_from(snapshot.whole(EMBEDDING_TIMEOUT_MS)).unwrap_or(15_000),
            ),
        }
    }

    pub fn enabled(&self) -> bool {
        self.provider.is_some()
    }

    /// Nome do modelo; no `local`, o canônico do `fastembed`.
    pub fn model_name(&self) -> Option<String> {
        let model = self.model.clone()?;
        match self.provider? {
            EmbeddingProvider::Local => local::find(&model).map(|model| model.id),
            _ => Some(model),
        }
    }

    /// `<provider>:<model>[:<dimensions>]` (007 D-001); `None` desligado ou incompleto.
    pub fn model_id(&self) -> Option<String> {
        let provider = self.provider?;
        let model = self.model_name()?;
        Some(match self.dimensions {
            Some(dims) => format!("{}:{model}:{dims}", provider.as_str()),
            None => format!("{}:{model}", provider.as_str()),
        })
    }

    /// Chaves que faltam para chamar o provedor.
    pub fn missing(&self) -> Vec<&'static str> {
        let Some(provider) = self.provider else {
            return vec![EMBEDDING_PROVIDER];
        };
        let mut missing = Vec::new();
        if self.model.is_none() {
            missing.push(EMBEDDING_MODEL);
        }
        if provider.requires_base_url() && self.base_url.is_none() {
            missing.push(EMBEDDING_BASE_URL);
        }
        if provider.requires_api_key() && self.api_key.is_none() {
            missing.push(EMBEDDING_API_KEY);
        }
        missing
    }

    /// `(consulta, documento)`: os configurados ou os automáticos do modelo.
    pub fn prefixes(&self) -> (String, String) {
        let (query, document) = auto_prefixes(self.model.as_deref().unwrap_or_default());
        (
            self.query_prefix
                .clone()
                .unwrap_or_else(|| query.to_owned()),
            self.document_prefix
                .clone()
                .unwrap_or_else(|| document.to_owned()),
        )
    }

    /// Dimensão esperada: a configurada ou a do modelo local.
    pub fn expected_dims(&self) -> Option<usize> {
        self.dimensions.or_else(|| match self.provider? {
            EmbeddingProvider::Local => local::find(self.model.as_deref()?).map(|m| m.dims),
            _ => None,
        })
    }

    pub fn lease(&self) -> Duration {
        self.timeout + LEASE_MARGIN
    }
}

/// Provedor pronto para chamar, com o modelo, os prefixos e a dimensão esperada.
#[derive(Clone)]
pub struct EmbeddingRuntime {
    pub provider: EmbeddingProvider,
    pub model: String,
    pub model_id: String,
    pub query_prefix: String,
    pub document_prefix: String,
    pub expected_dims: Option<usize>,
    pub timeout: Duration,
    embedder: Arc<dyn Embedder>,
}

impl EmbeddingRuntime {
    /// Tempo esgotado enquanto o modelo local ainda carrega é `Loading`, não `Timeout`.
    fn timeout_error(&self) -> EmbedError {
        match self.embedder.load_info(&self.model) {
            Some(info) if matches!(info.state, LoadState::Downloading | LoadState::Loading) => {
                EmbedError::Loading {
                    model: self.model.clone(),
                    state: info.state,
                    elapsed_ms: info
                        .elapsed
                        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
                        .unwrap_or_default(),
                }
            }
            _ => EmbedError::Timeout,
        }
    }

    /// Vetores na ordem dos textos, com o prefixo do tipo, dentro de `timeout`; confere
    /// quantidade, dimensão e valores.
    pub async fn embed(
        &self,
        texts: &[String],
        kind: EmbedKind,
        timeout: Duration,
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let prefix = match kind {
            EmbedKind::Query => &self.query_prefix,
            EmbedKind::Document => &self.document_prefix,
        };
        let inputs: Vec<String> = texts.iter().map(|text| format!("{prefix}{text}")).collect();
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(timeout, self.embedder.embed(&self.model, &inputs))
            .await
            .map_err(|_| self.timeout_error())
            .and_then(|result| result);
        crate::metrics::embedding(
            self.provider.as_str(),
            match kind {
                EmbedKind::Query => "query",
                EmbedKind::Document => "document",
            },
            started.elapsed().as_secs_f64(),
            result.is_ok(),
        );
        let vectors = result?;
        if vectors.len() != texts.len() {
            return Err(EmbedError::InvalidResponse);
        }
        let expected = self
            .expected_dims
            .or_else(|| vectors.first().map(Vec::len))
            .unwrap_or_default();
        for vector in &vectors {
            if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
                return Err(EmbedError::InvalidResponse);
            }
            if vector.len() != expected {
                return Err(EmbedError::Dimension {
                    expected,
                    got: vector.len(),
                });
            }
        }
        Ok(vectors)
    }
}

/// Similaridade de cosseno (de -1 a 1).
pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum();
    let norm = |v: &[f32]| v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    let denominator = norm(a) * norm(b);
    if denominator == 0.0 {
        0.0
    } else {
        dot / denominator
    }
}

/// Guarda de sentido do Redator (desenho §7): cosseno entre o original e o revisado, os dois
/// como documento, numa única chamada.
pub struct EmbeddingGuard {
    runtime: EmbeddingRuntime,
}

impl MeaningGuard for EmbeddingGuard {
    fn similarity<'a>(&'a self, original: &'a str, revised: &'a str) -> GuardFuture<'a> {
        Box::pin(async move {
            let texts = [original.to_owned(), revised.to_owned()];
            match self
                .runtime
                .embed(&texts, EmbedKind::Document, self.runtime.timeout)
                .await
            {
                Ok(vectors) => Some(cosine(&vectors[0], &vectors[1])),
                Err(error) => {
                    tracing::warn!(
                        event = "embeddings.guard_failed",
                        provider = self.runtime.provider.as_str(),
                        outcome = error.outcome(),
                        "guarda de sentido sem embedding; mantido o texto original"
                    );
                    None
                }
            }
        })
    }
}

fn short(error: &str) -> String {
    error.chars().take(MAX_ERROR_CHARS).collect()
}

pub(crate) fn not_configured(missing: &[&str]) -> ApiError {
    ApiError::new(
        "PROVIDER_NOT_CONFIGURED",
        "Embeddings desligados ou sem provedor, modelo, URL ou chave configurados",
    )
    .with_details(json!({ "missing": missing }))
}

fn unknown_local_model(model: &str) -> ApiError {
    ApiError::new(
        "SETTING_INVALID",
        format!("Modelo local de embedding desconhecido: {model}"),
    )
    .with_details(json!({
        "key": EMBEDDING_MODEL,
        "valid": local::supported().into_iter().map(|model| model.id).collect::<Vec<_>>(),
    }))
}

/// Erro de uma chamada explícita ao provedor (`test_embedding`, troca de modelo, listagem).
pub(crate) fn provider_error(error: &EmbedError) -> ApiError {
    match error {
        EmbedError::Timeout => ApiError::new(
            "PROVIDER_TIMEOUT",
            "O provedor de embedding nao respondeu dentro de embedding.timeout_ms",
        )
        .with_details(json!({ "outcome": "timeout" })),
        EmbedError::Dimension { expected, got } => ApiError::new(
            "EMBEDDING_DIMENSION_MISMATCH",
            format!("O provedor devolveu vetores de {got} dimensoes; esperadas {expected}"),
        )
        .with_details(json!({ "expected": expected, "got": got })),
        EmbedError::UnknownModel(model) => unknown_local_model(model),
        EmbedError::Loading {
            model,
            state,
            elapsed_ms,
        } => {
            let started_at = ids::now()
                .checked_sub_signed(chrono::Duration::milliseconds(
                    i64::try_from(*elapsed_ms).unwrap_or(i64::MAX),
                ))
                .map(ids::timestamp);
            ApiError::new(
                "EMBEDDING_MODEL_LOADING",
                format!(
                    "O modelo local de embedding {model} ainda esta em {}; acompanhe get_status.embedding.local e repita quando state = ready",
                    state.as_str()
                ),
            )
            .with_details(json!({
                "model": model,
                "state": state.as_str(),
                "started_at": started_at,
                "elapsed_ms": elapsed_ms,
            }))
        }
        EmbedError::Rejected(_) => ApiError::new(
            "PROVIDER_REJECTED",
            format!("O provedor de embedding recusou a requisicao: {error}"),
        )
        .with_details(json!({ "outcome": error.outcome(), "reason": short(&error.to_string()) })),
        other => ApiError::new(
            "PROVIDER_FAILED",
            format!("O provedor de embedding falhou: {other}"),
        )
        .with_details(json!({ "outcome": other.outcome(), "reason": short(&other.to_string()) })),
    }
}

/// Configuração proposta: as chaves `embedding.*` validadas e a chave em claro, à parte.
type Proposal = (BTreeMap<&'static str, Value>, Option<Zeroizing<String>>);

/// Valida um mapa proposto de chaves `embedding.*`; a chave em claro sai à parte.
fn proposed_settings(settings: Option<&BTreeMap<String, Value>>) -> Result<Proposal, ApiError> {
    let mut proposed = BTreeMap::new();
    let mut api_key = None;
    for (name, value) in settings.into_iter().flatten() {
        let spec = settings::spec(name)
            .filter(|spec| spec.key.starts_with("embedding."))
            .ok_or_else(|| {
                ApiError::new(
                    "SETTING_UNKNOWN",
                    format!("Configuracao desconhecida para os embeddings: {name}"),
                )
                .with_details(json!({
                    "key": name,
                    "valid": settings::SETTINGS
                        .iter()
                        .filter(|spec| spec.key.starts_with("embedding."))
                        .map(|spec| spec.key)
                        .collect::<Vec<_>>(),
                }))
            })?;
        if spec.key == EMBEDDING_API_KEY {
            match value {
                Value::String(secret) if !secret.trim().is_empty() => {
                    api_key = Some(Zeroizing::new(secret.trim().to_owned()));
                }
                _ => {
                    return Err(ApiError::new(
                        "SETTING_INVALID",
                        "embedding.api_key proposta deve ser um texto",
                    )
                    .with_details(json!({ "key": EMBEDDING_API_KEY, "type": "secret" })));
                }
            }
            continue;
        }
        if !value.is_null() {
            settings::validate(spec, value)?;
        }
        proposed.insert(spec.key, value.clone());
    }
    Ok((proposed, api_key))
}

// ---------- tipos das operações ----------

/// Corpo de `test_embedding`: um texto e, opcionalmente, uma configuração proposta (chaves
/// `embedding.*`, inclusive `embedding.api_key` em claro), que não é gravada.
#[derive(Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestEmbeddingBody {
    /// Texto de exemplo (1 a 2000 caracteres).
    #[schema(example = "Use cargo nextest para rodar os testes localmente.")]
    pub text: String,
    /// Mapa parcial `embedding.* → valor` usado só neste teste.
    #[serde(default)]
    #[schema(value_type = Option<Object>, example = json!({ "embedding.provider": "ollama", "embedding.base_url": "http://127.0.0.1:11434", "embedding.model": "bge-m3" }))]
    pub settings: Option<BTreeMap<String, Value>>,
}

impl std::fmt::Debug for TestEmbeddingBody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TestEmbeddingBody([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TestEmbeddingResult {
    #[schema(example = "local")]
    pub provider: String,
    #[schema(example = "multilingual-e5-small")]
    pub model: String,
    #[schema(example = "local:multilingual-e5-small")]
    pub model_id: String,
    /// Dimensão detectada no vetor devolvido.
    #[schema(example = 384)]
    pub dimensions: usize,
    #[schema(example = 21)]
    pub latency_ms: i64,
}

/// Corpo de `list_embedding_models`.
#[derive(Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListEmbeddingModelsBody {
    /// `local`, `openai_compatible`, `ollama` (`gemini` e `off` não listam).
    #[schema(example = "ollama")]
    pub provider: String,
    /// URL do provedor remoto; vazia usa `embedding.base_url`.
    #[schema(example = "http://127.0.0.1:11434")]
    pub base_url: Option<String>,
    /// Chave em claro (não é gravada); vazia usa `embedding.api_key`.
    pub api_key: Option<String>,
}

impl std::fmt::Debug for ListEmbeddingModelsBody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ListEmbeddingModelsBody")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EmbeddingModelList {
    pub models: Vec<EmbeddingModelEntry>,
}

/// Bloco `embedding` de `get_status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EmbeddingStatus {
    /// `embedding.provider != off`.
    pub enabled: bool,
    /// Provedor, modelo, URL e chave presentes.
    pub configured: bool,
    #[schema(example = json!([]))]
    pub missing: Vec<String>,
    #[schema(example = "local")]
    pub provider: Option<String>,
    #[schema(example = "multilingual-e5-small")]
    pub model: Option<String>,
    #[schema(example = "local:multilingual-e5-small")]
    pub model_id: Option<String>,
    /// `off`, `ready`, `reindexing` ou `failed` (lote da reindexação falhou de vez).
    #[schema(example = "ready")]
    pub state: String,
    /// Dimensão esperada (a configurada ou a do modelo local); `null` até a primeira chamada
    /// nos remotos sem `embedding.dimensions`.
    #[schema(example = 384)]
    pub dimensions: Option<usize>,
    /// Memórias a vetorizar: todas, ou as da reindexação em andamento.
    #[schema(example = 1234)]
    pub total: u64,
    /// Já vetorizadas com o modelo vigente (na reindexação, desde o início dela).
    #[schema(example = 1234)]
    pub done: u64,
    /// Início da reindexação em andamento.
    pub reindex_started_at: Option<String>,
    /// Download e carregamento do modelo local; `null` fora do provedor `local`.
    pub local: Option<LocalModelStatus>,
}

/// Estado do modelo local de embedding em `get_status.embedding.local`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LocalModelStatus {
    #[schema(example = "multilingual-e5-large")]
    pub model: String,
    /// `idle` (ainda não pedido), `downloading` (baixando para `MODELS_PATH`), `loading`
    /// (carregando do cache), `ready` ou `failed`.
    #[schema(example = "ready")]
    pub state: String,
    /// Início do download ou carregamento.
    pub started_at: Option<String>,
    /// Tempo decorrido; em `ready` e `failed`, quanto levou.
    #[schema(example = 67413)]
    pub elapsed_ms: Option<u64>,
    /// Motivo da falha (`failed`); a próxima tentativa começa 30 s depois, na próxima chamada.
    pub error: Option<String>,
}

// ---------- Core ----------

impl<S: Store> Core<S> {
    /// Configuração de embedding com a chave decifrada.
    pub(crate) async fn embedding_config(
        &self,
        snapshot: &SettingsSnapshot,
    ) -> Result<EmbeddingConfig, ApiError> {
        Ok(EmbeddingConfig::new(
            snapshot,
            self.secret(EMBEDDING_API_KEY).await?,
        ))
    }

    /// Embeddings ligados (`embedding.provider != off`): a escrita enfileira o `embed`.
    pub(crate) fn embeddings_enabled(&self) -> bool {
        self.settings_snapshot()
            .optional_text(EMBEDDING_PROVIDER)
            .as_deref()
            != Some("off")
    }

    /// Provedor pronto para chamar; `Err` com as chaves que faltam (ou o modelo local
    /// desconhecido).
    pub(crate) fn embedding_runtime(
        &self,
        config: &EmbeddingConfig,
    ) -> Result<EmbeddingRuntime, ApiError> {
        let missing = config.missing();
        if !missing.is_empty() {
            return Err(not_configured(&missing));
        }
        let provider = config
            .provider
            .ok_or_else(|| not_configured(&[EMBEDDING_PROVIDER]))?;
        let model = config
            .model_name()
            .ok_or_else(|| unknown_local_model(config.model.as_deref().unwrap_or_default()))?;
        let model_id = config
            .model_id()
            .ok_or_else(|| not_configured(&[EMBEDDING_MODEL]))?;
        let embedder: Arc<dyn Embedder> = match provider {
            EmbeddingProvider::Local => self.local_embedder(),
            remote => {
                let base_url = config
                    .base_url
                    .clone()
                    .or_else(|| remote.default_base_url().map(str::to_owned))
                    .ok_or_else(|| not_configured(&[EMBEDDING_BASE_URL]))?;
                let endpoint = Endpoint::new(
                    self.http().clone(),
                    &base_url,
                    config.api_key.clone(),
                    config.timeout,
                );
                match remote {
                    EmbeddingProvider::OpenAiCompatible => {
                        Arc::new(OpenAiCompatible::new(endpoint, config.dimensions))
                    }
                    EmbeddingProvider::Ollama => Arc::new(Ollama::new(endpoint)),
                    _ => Arc::new(Gemini::new(endpoint, config.dimensions)),
                }
            }
        };
        let (query_prefix, document_prefix) = config.prefixes();
        Ok(EmbeddingRuntime {
            provider,
            model,
            model_id,
            query_prefix,
            document_prefix,
            expected_dims: config.expected_dims(),
            timeout: config.timeout,
            embedder,
        })
    }

    /// Provedor da configuração vigente, ou `None` (desligado, incompleto ou desconhecido).
    pub(crate) async fn current_runtime(&self) -> Option<EmbeddingRuntime> {
        let config = self
            .embedding_config(&self.settings_snapshot())
            .await
            .ok()?;
        if !config.enabled() {
            return None;
        }
        self.embedding_runtime(&config).ok()
    }

    /// Vetor da consulta com o prazo curto da busca; `None` sem ramo vetorial (com `WARN`
    /// quando o provedor falhou).
    pub(crate) async fn query_vector(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Option<(EmbeddingRuntime, Vec<f32>)> {
        let runtime = self.current_runtime().await?;
        let timeout = QUERY_TIMEOUT.min(runtime.timeout);
        match runtime
            .embed(&[text.to_owned()], EmbedKind::Query, timeout)
            .await
        {
            Ok(mut vectors) => {
                let vector = vectors.pop()?;
                Some((runtime, vector))
            }
            Err(error) => {
                tracing::warn!(
                    event = "embeddings.query_failed",
                    operation,
                    provider = runtime.provider.as_str(),
                    outcome = error.outcome(),
                    "consulta sem embedding; busca segue so com o FTS"
                );
                None
            }
        }
    }

    /// Guarda de sentido com a configuração vigente (ou a dos testes).
    pub(crate) async fn embedding_guard(&self) -> Option<Arc<dyn MeaningGuard>> {
        let runtime = self.current_runtime().await?;
        Some(Arc::new(EmbeddingGuard { runtime }))
    }

    /// Compara a configuração atual com a proposta; quando o espaço dos vetores muda, valida
    /// o provedor (fora da transação) e devolve a troca a gravar junto (FR-008).
    pub(crate) async fn embedding_switch(
        &self,
        current: &SettingsSnapshot,
        proposed: &SettingsSnapshot,
    ) -> Result<Option<EmbeddingSwitch>, ApiError> {
        let changed = EMBEDDING_IDENTITY_KEYS
            .iter()
            .any(|key| current.value(key) != proposed.value(key));
        if !changed {
            return Ok(None);
        }
        let before = self.embedding_config(current).await?;
        let after = self.embedding_config(proposed).await?;
        if !after.enabled() {
            return Ok(Some(EmbeddingSwitch {
                model_id: None,
                reindex: None,
            }));
        }
        let runtime = self.embedding_runtime(&after)?;
        if runtime.provider != EmbeddingProvider::Local {
            // O local só confere o modelo (o download fica para o primeiro job, Q-018).
            runtime
                .embed(&[SAMPLE.to_owned()], EmbedKind::Document, after.timeout)
                .await
                .map_err(|error| provider_error(&error))?;
        }
        let same_space = before.enabled() && before.model_id() == after.model_id();
        Ok(Some(EmbeddingSwitch {
            model_id: Some(runtime.model_id),
            reindex: Some(ReindexPlan {
                // Mesmo `model_id` com outra URL ou outro prefixo: os vetores antigos não
                // servem mais; com outro `model_id`, só falta o que não tem vetor dele.
                all: same_space,
                batch_size: after.batch_size,
                started_at: ids::timestamp(ids::now()),
            }),
        }))
    }

    /// Depois de gravar uma troca: acorda a fila, ou encerra na hora sem nada a reindexar.
    pub(crate) async fn embedding_switched(&self, switch: &EmbeddingSwitch) {
        let Some(model_id) = &switch.model_id else {
            return;
        };
        tracing::info!(
            event = "embeddings.reindex_started",
            model_id = %model_id,
            "configuracao de embedding trocada"
        );
        match self.store().reindex_state().await {
            Ok(Some(_)) => self.jobs_enqueued(),
            Ok(None) => self.finish_reindex(model_id, None).await,
            Err(error) => tracing::warn!(
                event = "embeddings.reindex_state_failed",
                error = %error,
                "falha ao ler a reindexacao"
            ),
        }
    }

    /// Encerra a reindexação (`started_at`) ou só limpa (`None`): apaga vetores de outros
    /// modelos e os modelos locais fora de uso (Q-002).
    pub(crate) async fn finish_reindex(&self, model_id: &str, started_at: Option<String>) {
        match self
            .store()
            .finish_reindex(model_id.to_owned(), started_at.clone())
            .await
        {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::warn!(event = "embeddings.reindex_finish_failed", error = %error, "falha ao encerrar a reindexacao");
                return;
            }
        }
        if started_at.is_some() {
            tracing::info!(
                event = "embeddings.reindex_finished",
                model_id = %model_id,
                "reindexacao concluida"
            );
        }
        let keep = model_id
            .strip_prefix("local:")
            .and_then(|rest| local::find(rest.split(':').next().unwrap_or(rest)));
        let path = self.models_path().to_path_buf();
        let removed =
            tokio::task::spawn_blocking(move || local::remove_unused(&path, keep.as_ref())).await;
        match removed {
            Ok(Ok(removed)) => {
                for model in removed {
                    tracing::info!(event = "embeddings.model_removed", model = %model, "modelo local fora de uso removido");
                }
            }
            Ok(Err(error)) => tracing::warn!(
                event = "embeddings.model_remove_failed",
                error = %error,
                "falha ao remover modelo local fora de uso"
            ),
            Err(_) => {}
        }
    }

    /// Inicialização: grava o `model_id` vigente (sem reindexar) e encerra uma reindexação
    /// que terminou enquanto o serviço estava parado.
    pub async fn sync_embeddings(&self) -> Result<(), StoreError> {
        let config = EmbeddingConfig::new(&self.settings_snapshot(), None);
        let current = if config.enabled() {
            config.model_id().unwrap_or_default()
        } else {
            String::new()
        };
        if self.store().embedding_model().await?.as_deref() != Some(current.as_str()) {
            self.store().set_embedding_model(current.clone()).await?;
        }
        if let Some(state) = self.store().reindex_state().await?
            && state.model_id == current
        {
            let jobs = self
                .store()
                .reindex_jobs(state.started_at.clone(), None)
                .await?;
            if jobs.pending == 0 && jobs.failed == 0 {
                self.finish_reindex(&current, Some(state.started_at)).await;
            }
        }
        Ok(())
    }

    pub async fn test_embedding(
        &self,
        principal: &Principal,
        body: TestEmbeddingBody,
        key: Option<&str>,
    ) -> Result<Mutation<TestEmbeddingResult>, ApiError> {
        let operation = self.authorize(principal, "test_embedding")?;
        let text = body.text.trim().to_owned();
        if text.is_empty() || text.chars().count() > 2000 {
            return Err(ApiError::validation("text deve ter de 1 a 2000 caracteres")
                .with_details(json!({ "field": "text" })));
        }
        let (proposed, proposed_key) = proposed_settings(body.settings.as_ref())?;
        let snapshot = self.settings_snapshot().with(&proposed);
        let mut config = self.embedding_config(&snapshot).await?;
        if proposed_key.is_some() {
            config.api_key = proposed_key;
        }
        let runtime = self.embedding_runtime(&config)?;
        let core = self.clone();
        self.mutate(
            principal,
            operation,
            key,
            &body,
            200,
            |context| async move {
                let started = Instant::now();
                let vectors = runtime
                    .embed(&[text], EmbedKind::Document, runtime.timeout)
                    .await
                    .map_err(|error| provider_error(&error))?;
                let result = TestEmbeddingResult {
                    provider: runtime.provider.as_str().to_owned(),
                    model: runtime.model.clone(),
                    model_id: runtime.model_id.clone(),
                    dimensions: vectors.first().map(Vec::len).unwrap_or_default(),
                    latency_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
                };
                if context.idempotency.is_some() {
                    let response =
                        serde_json::to_value(&result).map_err(|_| ApiError::internal())?;
                    core.store()
                        .append_llm_calls(Vec::new(), Some((context, response)))
                        .await
                        .map_err(|error| super::store_error(error, ApiError::internal))?;
                }
                Ok(result)
            },
        )
        .await
    }

    pub async fn list_embedding_models(
        &self,
        principal: &Principal,
        body: ListEmbeddingModelsBody,
        key: Option<&str>,
    ) -> Result<Mutation<EmbeddingModelList>, ApiError> {
        let operation = self.authorize(principal, "list_embedding_models")?;
        let name = body.provider.trim();
        let provider = match name {
            "gemini" | "off" => {
                return Err(ApiError::new(
                    "PROVIDER_LISTING_UNSUPPORTED",
                    format!("O provedor {name} nao oferece listagem de modelos de embedding"),
                )
                .with_details(json!({ "provider": name })));
            }
            other => EmbeddingProvider::parse(other).ok_or_else(|| {
                ApiError::validation(
                    "provider deve ser local, openai_compatible, ollama, gemini ou off",
                )
                .with_details(json!({ "field": "provider" }))
            })?,
        };
        let input = json!({ "provider": name, "base_url": body.base_url });
        let endpoint = if provider == EmbeddingProvider::Local {
            None
        } else {
            let snapshot = self.settings_snapshot();
            let base_url = match body
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
            {
                Some(url) => {
                    let spec = settings::spec(EMBEDDING_BASE_URL).ok_or_else(ApiError::internal)?;
                    settings::validate(spec, &Value::String(url.to_owned()))?;
                    Some(url.to_owned())
                }
                None => snapshot.optional_text(EMBEDDING_BASE_URL),
            }
            .ok_or_else(|| not_configured(&[EMBEDDING_BASE_URL]))?;
            let api_key = match body
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|k| !k.is_empty())
            {
                Some(key) => Some(Zeroizing::new(key.to_owned())),
                None => self.secret(EMBEDDING_API_KEY).await?,
            };
            let timeout = EmbeddingConfig::new(&snapshot, None).timeout;
            Some(Endpoint::new(
                self.http().clone(),
                &base_url,
                api_key,
                timeout,
            ))
        };
        let core = self.clone();
        self.mutate(
            principal,
            operation,
            key,
            &input,
            200,
            |context| async move {
                let models = match &endpoint {
                    None => local::supported()
                        .iter()
                        .map(local::LocalModel::entry)
                        .collect(),
                    Some(endpoint) => list_remote_models(provider, endpoint)
                        .await
                        .map_err(|error| provider_error(&error))?,
                };
                let result = EmbeddingModelList { models };
                if context.idempotency.is_some() {
                    let response =
                        serde_json::to_value(&result).map_err(|_| ApiError::internal())?;
                    core.store()
                        .append_llm_calls(Vec::new(), Some((context, response)))
                        .await
                        .map_err(|error| super::store_error(error, ApiError::internal))?;
                }
                Ok(result)
            },
        )
        .await
    }

    pub async fn reembed_all(
        &self,
        principal: &Principal,
        key: Option<&str>,
    ) -> Result<Mutation<ReindexState>, ApiError> {
        let operation = self.authorize(principal, "reembed_all")?;
        let config = self.embedding_config(&self.settings_snapshot()).await?;
        let runtime = self.embedding_runtime(&config)?;
        let core = self.clone();
        let model_id = runtime.model_id.clone();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &json!({}),
                202,
                |context| async move {
                    let plan = ReindexPlan {
                        all: true,
                        batch_size: config.batch_size,
                        started_at: context.at.clone(),
                    };
                    match core.store().start_reindex(model_id, plan, context).await {
                        Err(StoreError::Conflict("REEMBED_IN_PROGRESS")) => Err(ApiError::new(
                            "REEMBED_IN_PROGRESS",
                            "Ja ha uma reindexacao em andamento; acompanhe em get_status",
                        )),
                        other => {
                            other.map_err(|error| super::store_error(error, ApiError::internal))
                        }
                    }
                },
            )
            .await?;
        if !result.replayed {
            tracing::info!(
                event = "embeddings.reindex_started",
                model_id = %result.value.model_id,
                total = result.value.total,
                "reembed_all enfileirado"
            );
            if result.value.total == 0 {
                self.finish_reindex(&result.value.model_id, None).await;
            } else {
                self.jobs_enqueued();
            }
        }
        Ok(result)
    }

    /// Bloco `embedding` de `get_status`.
    /// Com `embedding.provider = local`, baixa e carrega o modelo vigente em segundo plano,
    /// para que a primeira chamada não espere o download.
    pub fn prepare_local_embedding(&self) {
        let snapshot = self.settings_snapshot();
        if snapshot.optional_text(EMBEDDING_PROVIDER).as_deref() != Some("local") {
            return;
        }
        if let Some(model) = snapshot.optional_text(EMBEDDING_MODEL) {
            self.local_embedder().prepare(&model);
        }
    }

    pub(crate) async fn embedding_status(&self) -> Result<EmbeddingStatus, ApiError> {
        let config = self.embedding_config(&self.settings_snapshot()).await?;
        let missing = config.missing();
        let model_id = config.model_id();
        let mut status = EmbeddingStatus {
            enabled: config.enabled(),
            configured: config.enabled() && missing.is_empty(),
            missing: missing.iter().map(|key| (*key).to_owned()).collect(),
            provider: config.provider.map(|provider| provider.as_str().to_owned()),
            model: config.model_name().or_else(|| config.model.clone()),
            model_id: model_id.clone(),
            state: "off".to_owned(),
            dimensions: config.expected_dims(),
            total: 0,
            done: 0,
            reindex_started_at: None,
            local: None,
        };
        if config.provider == Some(EmbeddingProvider::Local)
            && let Some(model) = status.model.clone()
            && let Some(info) = self.local_embedder().load_info(&model)
        {
            status.local = Some(LocalModelStatus {
                model,
                state: info.state.as_str().to_owned(),
                started_at: info
                    .started
                    .map(|started| ids::timestamp(chrono::DateTime::<chrono::Utc>::from(started))),
                elapsed_ms: info
                    .elapsed
                    .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)),
                error: info.error,
            });
        }
        if !config.enabled() {
            status.missing.clear();
            return Ok(status);
        }
        let Some(model_id) = model_id else {
            status.state = "ready".to_owned();
            return Ok(status);
        };
        let reindex = self
            .store()
            .reindex_state()
            .await?
            .filter(|state| state.model_id == model_id);
        let counts = self
            .store()
            .vector_counts(
                model_id.clone(),
                reindex.as_ref().map(|state| state.started_at.clone()),
            )
            .await?;
        status.state = "ready".to_owned();
        status.total = counts.memories;
        status.done = counts.vectors.min(counts.memories);
        if let Some(reindex) = reindex {
            let jobs = self
                .store()
                .reindex_jobs(reindex.started_at.clone(), None)
                .await?;
            if jobs.pending > 0 {
                status.state = "reindexing".to_owned();
            } else if jobs.failed > 0 {
                status.state = "failed".to_owned();
            }
            if status.state != "ready" {
                status.total = reindex.total;
                status.done = counts.since.min(reindex.total);
                status.reindex_started_at = Some(reindex.started_at);
            }
        }
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(entries: &[(&'static str, Value)]) -> EmbeddingConfig {
        let proposed: BTreeMap<&'static str, Value> = entries.iter().cloned().collect();
        EmbeddingConfig::new(&SettingsSnapshot::default().with(&proposed), None)
    }

    #[test]
    fn defaults_are_local_e5_with_automatic_prefixes() {
        let config = config(&[]);
        assert!(config.enabled());
        assert_eq!(
            config.model_id().as_deref(),
            Some("local:multilingual-e5-small")
        );
        assert_eq!(config.missing(), Vec::<&str>::new());
        assert_eq!(
            config.prefixes(),
            ("query: ".to_owned(), "passage: ".to_owned())
        );
        assert_eq!(config.expected_dims(), Some(384));
        assert_eq!(config.batch_size, 32);
        assert_eq!(config.lease(), Duration::from_secs(75));
    }

    #[test]
    fn model_id_carries_configured_dimensions_and_prefixes_can_be_overridden() {
        let config = config(&[
            (EMBEDDING_PROVIDER, json!("openai_compatible")),
            (EMBEDDING_MODEL, json!("text-embedding-3-small")),
            (EMBEDDING_DIMENSIONS, json!(512)),
            (EMBEDDING_QUERY_PREFIX, json!("q: ")),
        ]);
        assert_eq!(
            config.model_id().as_deref(),
            Some("openai_compatible:text-embedding-3-small:512")
        );
        assert_eq!(config.missing(), vec![EMBEDDING_BASE_URL]);
        assert_eq!(config.prefixes(), ("q: ".to_owned(), String::new()));
        assert_eq!(config.expected_dims(), Some(512));
    }

    #[test]
    fn off_and_incomplete_configurations() {
        let off = config(&[(EMBEDDING_PROVIDER, json!("off"))]);
        assert!(!off.enabled());
        assert_eq!(off.model_id(), None);
        assert_eq!(off.missing(), vec![EMBEDDING_PROVIDER]);
        let gemini = config(&[
            (EMBEDDING_PROVIDER, json!("gemini")),
            (EMBEDDING_MODEL, json!("gemini-embedding-001")),
        ]);
        assert_eq!(gemini.missing(), vec![EMBEDDING_API_KEY]);
        let unknown = config(&[(EMBEDDING_MODEL, json!("gpt-4o"))]);
        assert_eq!(unknown.model_id(), None);
        let canonical = config(&[(EMBEDDING_MODEL, json!("MultilingualE5Small"))]);
        assert_eq!(
            canonical.model_id().as_deref(),
            Some("local:multilingual-e5-small")
        );
    }

    #[test]
    fn cosine_is_bounded_and_safe_with_zero_vectors() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-9);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn proposed_settings_only_accept_embedding_keys() {
        let mut map = BTreeMap::new();
        map.insert("embedding.model".to_owned(), json!("bge-m3"));
        map.insert("embedding.api_key".to_owned(), json!("segredo"));
        let (proposed, key) = proposed_settings(Some(&map)).expect("ok");
        assert_eq!(proposed.get(EMBEDDING_MODEL), Some(&json!("bge-m3")));
        assert_eq!(key.as_deref().map(String::as_str), Some("segredo"));
        map.insert("redator.model".to_owned(), json!("x"));
        assert_eq!(
            proposed_settings(Some(&map)).expect_err("unknown").code,
            "SETTING_UNKNOWN"
        );
    }
}
