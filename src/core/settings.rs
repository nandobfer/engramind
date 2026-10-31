//! Configurações funcionais gerenciadas pela API (desenho §12, D-013). O catálogo `SETTINGS`
//! define chave, tipo, padrão, validação e sobreposição por canal; é a base do teste de
//! contrato 6.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};

use super::{
    Core,
    idempotency::Mutation,
    model::{Channel, SecretView, SettingChange, SettingRow},
    principal::Principal,
};
use crate::{
    error::ApiError,
    store::{Store, StoreError},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingType {
    Integer {
        min: i64,
        max: i64,
    },
    Number {
        min: f64,
        max: f64,
    },
    Boolean,
    /// Lista `{ key, label, priority }` de 1 a `MAX_CATEGORIES` itens.
    Categories,
    /// Um dos textos da lista.
    Choice(&'static [&'static str]),
    /// Texto de 1 a `max` caracteres.
    Text {
        max: usize,
    },
    /// URL `http://` ou `https://`.
    Url,
    /// Lista de até `max_items` textos de 1 a `max_len` caracteres.
    TextList {
        max_items: usize,
        max_len: usize,
    },
    /// Só gravado por `set_secret` (cifrado); a leitura mostra `{ configured, updated_at }`.
    Secret,
}

impl SettingType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Integer { .. } => "integer",
            Self::Number { .. } => "number",
            Self::Boolean => "boolean",
            Self::Categories => "categories",
            Self::Choice(_) => "enum",
            Self::Text { .. } => "text",
            Self::Url => "url",
            Self::TextList { .. } => "list",
            Self::Secret => "secret",
        }
    }

    /// Intervalo como o guia `configuracoes` o mostra.
    pub fn range(self) -> String {
        match self {
            Self::Integer { min, max } => format!("{min} a {max}"),
            Self::Number { min, max } => format!("{min} a {max}"),
            Self::Boolean => "—".to_owned(),
            Self::Categories => format!("1 a {MAX_CATEGORIES} itens"),
            Self::Choice(choices) => choices
                .iter()
                .map(|choice| format!("`{choice}`"))
                .collect::<Vec<_>>()
                .join(", "),
            Self::Text { max } => format!("1 a {max} caracteres"),
            Self::Url => "`http://` ou `https://`".to_owned(),
            Self::TextList { max_items, max_len } => {
                format!("até {max_items} itens de 1 a {max_len} caracteres")
            }
            Self::Secret => "só gravado (`set_secret`)".to_owned(),
        }
    }

    fn bounds(self) -> (Option<serde_json::Number>, Option<serde_json::Number>) {
        match self {
            Self::Integer { min, max } => (Some(min.into()), Some(max.into())),
            Self::Number { min, max } => (
                serde_json::Number::from_f64(min),
                serde_json::Number::from_f64(max),
            ),
            Self::Boolean | Self::Choice(_) | Self::Url | Self::Secret => (None, None),
            Self::Categories => (Some(1.into()), Some((MAX_CATEGORIES as u64).into())),
            Self::Text { max } => (Some(1.into()), Some((max as u64).into())),
            Self::TextList { max_items, .. } => (Some(0.into()), Some((max_items as u64).into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SettingSpec {
    pub key: &'static str,
    pub kind: SettingType,
    /// Padrão em JSON.
    pub default: &'static str,
    pub channel_override: bool,
    pub description: &'static str,
    /// Versão em que a chave entrou.
    pub since: &'static str,
}

pub const RATE_PER_MINUTE: &str = "limits.rate_per_minute";
pub const MAX_BATCH: &str = "limits.max_batch";
pub const CATEGORIES: &str = "categories.list";
pub const TEXT_MIN: &str = "memory.text_min";
pub const TEXT_MAX: &str = "memory.text_max";
pub const DEDUPE_EXACT: &str = "memory.dedupe_exact";
pub const SIMILAR_THRESHOLD: &str = "memory.similar_threshold";
pub const BUDGET_TOKENS: &str = "context.budget_tokens";
pub const TITLES_BUDGET_TOKENS: &str = "context.titles_budget_tokens";
pub const DEFAULT_FORMAT: &str = "context.default_format";
pub const TOKEN_ESTIMATOR: &str = "context.token_estimator";
pub const OVERFLOW_LINE: &str = "context.overflow_line";
pub const REDATOR_ENABLED: &str = "redator.enabled";
pub const REDATOR_PROVIDER: &str = "redator.provider";
pub const REDATOR_MODEL: &str = "redator.model";
pub const REDATOR_API_KEY: &str = "redator.api_key";
pub const REDATOR_BASE_URL: &str = "redator.base_url";
pub const REDATOR_INSTRUCTIONS: &str = "redator.instructions";
pub const REDATOR_REWRITE_TEXT: &str = "redator.rewrite_text";
pub const REDATOR_TITLE_MAX: &str = "redator.title_max";
pub const REDATOR_MIN_SIMILARITY: &str = "redator.min_similarity";
pub const REDATOR_TIMEOUT_MS: &str = "redator.timeout_ms";
pub const REDATOR_MAX_ATTEMPTS: &str = "redator.max_attempts";
pub const REDATOR_FALLBACK_MODELS: &str = "redator.fallback_models";
pub const EMBEDDING_PROVIDER: &str = "embedding.provider";
pub const EMBEDDING_MODEL: &str = "embedding.model";
pub const EMBEDDING_BASE_URL: &str = "embedding.base_url";
pub const EMBEDDING_API_KEY: &str = "embedding.api_key";
pub const EMBEDDING_DIMENSIONS: &str = "embedding.dimensions";
pub const EMBEDDING_QUERY_PREFIX: &str = "embedding.query_prefix";
pub const EMBEDDING_DOCUMENT_PREFIX: &str = "embedding.document_prefix";
pub const EMBEDDING_BATCH_SIZE: &str = "embedding.batch_size";
pub const EMBEDDING_TIMEOUT_MS: &str = "embedding.timeout_ms";
pub const SEARCH_VECTOR_WEIGHT: &str = "search.vector_weight";
pub const SEARCH_TEXT_WEIGHT: &str = "search.text_weight";
pub const SEARCH_RRF_K: &str = "search.rrf_k";
pub const SEARCH_DEFAULT_LIMIT: &str = "search.default_limit";
pub const SEARCH_HINT_THRESHOLD: &str = "search.hint_threshold";
pub const SEARCH_HINT_LIMIT: &str = "search.hint_limit";
pub const WEBHOOKS_ENABLED: &str = "webhooks.enabled";
pub const WEBHOOKS_TIMEOUT_MS: &str = "webhooks.timeout_ms";
pub const WEBHOOKS_MAX_ATTEMPTS: &str = "webhooks.max_attempts";
pub const RETENTION_USAGE_DAYS: &str = "retention.usage_days";
pub const RETENTION_LLM_CALLS_DAYS: &str = "retention.llm_calls_days";
pub const RETENTION_AUDIT_DAYS: &str = "retention.audit_days";
pub const RETENTION_JOBS_DAYS: &str = "retention.jobs_days";
/// Provedores de embedding (`embedding.provider`).
pub const EMBEDDING_PROVIDERS: &[&str] = &["local", "openai_compatible", "ollama", "gemini", "off"];
/// Chaves que definem o espaço dos vetores: mudar uma delas valida o provedor e reindexa.
pub const EMBEDDING_IDENTITY_KEYS: &[&str] = &[
    EMBEDDING_PROVIDER,
    EMBEDDING_MODEL,
    EMBEDDING_BASE_URL,
    EMBEDDING_DIMENSIONS,
    EMBEDDING_QUERY_PREFIX,
    EMBEDDING_DOCUMENT_PREFIX,
];
/// Provedores do Redator (`redator.provider`).
pub const REDATOR_PROVIDERS: &[&str] = &["openai_compatible", "anthropic", "gemini"];

/// Chaves que mudam o bloco do contexto (entram no hash da `version`, 005 Q-008).
pub const CONTEXT_RENDER_KEYS: &[&str] = &[
    BUDGET_TOKENS,
    TITLES_BUDGET_TOKENS,
    TOKEN_ESTIMATOR,
    OVERFLOW_LINE,
    CATEGORIES,
];

pub const MAX_CATEGORIES: usize = 50;
const MAX_CATEGORY_KEY: usize = 40;
const MAX_CATEGORY_LABEL: usize = 80;

pub const SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        key: RATE_PER_MINUTE,
        kind: SettingType::Integer {
            min: 1,
            max: 100_000,
        },
        default: "600",
        channel_override: false,
        description: "Requisições por minuto aceitas de cada token; acima disso, 429 RATE_LIMITED.",
        since: "0.2.0",
    },
    SettingSpec {
        key: MAX_BATCH,
        kind: SettingType::Integer { min: 1, max: 1000 },
        default: "100",
        channel_override: false,
        description: "Itens aceitos por operação em lote.",
        since: "0.2.0",
    },
    SettingSpec {
        key: CATEGORIES,
        kind: SettingType::Categories,
        default: r#"[{"key":"PROJECT_RULES","label":"Regras do projeto","priority":50},{"key":"ARCHITECTURE","label":"Arquitetura","priority":40},{"key":"CONSTRAINTS","label":"Restrições","priority":30},{"key":"CONFIG_VALUES","label":"Valores de configuração","priority":20},{"key":"NAMING","label":"Nomenclatura","priority":10}]"#,
        channel_override: false,
        description: "Categorias aceitas nas memórias, `{ key, label, priority }`; a primeira é a padrão. Retirar uma categoria em uso dá 409 CATEGORY_IN_USE.",
        since: "0.4.0",
    },
    SettingSpec {
        key: TEXT_MIN,
        kind: SettingType::Integer { min: 1, max: 1000 },
        default: "3",
        channel_override: false,
        description: "Caracteres mínimos do texto de uma memória.",
        since: "0.4.0",
    },
    SettingSpec {
        key: TEXT_MAX,
        kind: SettingType::Integer {
            min: 10,
            max: 20_000,
        },
        default: "4000",
        channel_override: true,
        description: "Caracteres máximos do texto de uma memória.",
        since: "0.4.0",
    },
    SettingSpec {
        key: DEDUPE_EXACT,
        kind: SettingType::Boolean,
        default: "true",
        channel_override: false,
        description: "Texto igual (`content_hash` normalizado) a outra memória do canal devolve a existente com `duplicate: true`.",
        since: "0.4.0",
    },
    SettingSpec {
        key: SIMILAR_THRESHOLD,
        kind: SettingType::Number { min: 0.0, max: 1.0 },
        default: "0.85",
        channel_override: false,
        description: "Similaridade mínima para uma memória aparecer em `similar` na resposta de escrita (Jaccard dos termos dos candidatos do FTS; o cosseno entra com a consolidação).",
        since: "0.4.0",
    },
    SettingSpec {
        key: BUDGET_TOKENS,
        kind: SettingType::Integer {
            min: 0,
            max: 200_000,
        },
        default: "4000",
        channel_override: true,
        description: "Tokens da faixa de texto completo do contexto. No canal, é o teto da parte dele nessa faixa; o global continua valendo para o bloco.",
        since: "0.5.0",
    },
    SettingSpec {
        key: TITLES_BUDGET_TOKENS,
        kind: SettingType::Integer {
            min: 0,
            max: 50_000,
        },
        default: "800",
        channel_override: true,
        description: "Tokens da faixa `#id: título` do contexto. No canal, é o teto da parte dele nessa faixa.",
        since: "0.5.0",
    },
    SettingSpec {
        key: DEFAULT_FORMAT,
        kind: SettingType::Choice(&["tagged", "markdown", "json"]),
        default: r#""tagged""#,
        channel_override: false,
        description: "Formato do contexto quando `format` não é informado (e do resource `engramind://context`).",
        since: "0.5.0",
    },
    SettingSpec {
        key: TOKEN_ESTIMATOR,
        kind: SettingType::Choice(&["o200k", "cl100k"]),
        default: r#""o200k""#,
        channel_override: false,
        description: "Codificação usada para estimar os tokens do contexto (`o200k_base` ou `cl100k_base`).",
        since: "0.5.0",
    },
    SettingSpec {
        key: OVERFLOW_LINE,
        kind: SettingType::Boolean,
        default: "true",
        channel_override: false,
        description: "Mostra a linha de excedente (`<overflow count=\"N\">`) quando sobram memórias fora das duas faixas.",
        since: "0.5.0",
    },
    SettingSpec {
        key: REDATOR_ENABLED,
        kind: SettingType::Boolean,
        default: "true",
        channel_override: false,
        description: "Liga o Redator. Desligado (ou sem provedor, modelo e chave), toda memória nova recebe `title_state = fallback`, sem chamar o LLM.",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_PROVIDER,
        kind: SettingType::Choice(REDATOR_PROVIDERS),
        default: "null",
        channel_override: false,
        description: "Provedor do LLM: OpenAI-compatível (`/chat/completions`), Anthropic (`/v1/messages`) ou Gemini (`generateContent`).",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_MODEL,
        kind: SettingType::Text { max: 200 },
        default: "null",
        channel_override: false,
        description: "Modelo principal do Redator.",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_API_KEY,
        kind: SettingType::Secret,
        default: "null",
        channel_override: false,
        description: "Chave de API do provedor, cifrada com `SECRETS_KEY`. Grave com `set_secret`; opcional só no `openai_compatible` (servidores locais).",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_BASE_URL,
        kind: SettingType::Url,
        default: "null",
        channel_override: false,
        description: "URL base do provedor. Obrigatória no `openai_compatible` (por exemplo `https://api.openai.com/v1`); no `anthropic` e no `gemini`, vazia usa a URL oficial.",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_INSTRUCTIONS,
        kind: SettingType::Text { max: 4000 },
        default: "null",
        channel_override: true,
        description: "Instruções acrescentadas ao prompt do sistema do Redator (o canal substitui a global).",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_REWRITE_TEXT,
        kind: SettingType::Boolean,
        default: "true",
        channel_override: true,
        description: "O Redator também revisa o texto (nova revisão `system/redator`); `false` gera só o título.",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_TITLE_MAX,
        kind: SettingType::Integer { min: 20, max: 300 },
        default: "100",
        channel_override: false,
        description: "Caracteres máximos do título gerado; título maior invalida a resposta.",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_MIN_SIMILARITY,
        kind: SettingType::Number { min: 0.0, max: 1.0 },
        default: "0.8",
        channel_override: false,
        description: "Guarda de sentido: similaridade de cosseno mínima entre os embeddings do original e do revisado. Com `embedding.provider = off` ou o provedor indisponível, o texto revisado nunca é aplicado.",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_TIMEOUT_MS,
        kind: SettingType::Integer {
            min: 1000,
            max: 300_000,
        },
        default: "30000",
        channel_override: false,
        description: "Tempo limite de cada chamada ao LLM, em milissegundos.",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_MAX_ATTEMPTS,
        kind: SettingType::Integer { min: 1, max: 10 },
        default: "3",
        channel_override: false,
        description: "Tentativas do job `redact` (backoff de 2 s × 2ⁿ, teto de 10 min); esgotadas, `title_state = fallback`.",
        since: "0.6.0",
    },
    SettingSpec {
        key: REDATOR_FALLBACK_MODELS,
        kind: SettingType::TextList {
            max_items: 5,
            max_len: 200,
        },
        default: "[]",
        channel_override: false,
        description: "Modelos tentados em ordem quando o principal falha por indisponibilidade, limite (429) ou tempo limite.",
        since: "0.6.0",
    },
    SettingSpec {
        key: EMBEDDING_PROVIDER,
        kind: SettingType::Choice(EMBEDDING_PROVIDERS),
        default: r#""local""#,
        channel_override: false,
        description: "Quem gera os vetores: `local` (fastembed no próprio serviço), `openai_compatible` (`POST /embeddings`), `ollama` (`/api/embed`), `gemini` (`batchEmbedContents`) ou `off` (só busca textual). Trocar valida o provedor e reindexa.",
        since: "0.7.0",
    },
    SettingSpec {
        key: EMBEDDING_MODEL,
        kind: SettingType::Text { max: 200 },
        default: r#""multilingual-e5-small""#,
        channel_override: false,
        description: "Modelo de embedding (nunca de chat). No `local`, um dos de `list_embedding_models`. Trocar reindexa.",
        since: "0.7.0",
    },
    SettingSpec {
        key: EMBEDDING_BASE_URL,
        kind: SettingType::Url,
        default: "null",
        channel_override: false,
        description: "URL base do provedor remoto. Obrigatória no `openai_compatible` (por exemplo `https://api.openai.com/v1`) e no `ollama` (por exemplo `http://127.0.0.1:11434`); no `gemini`, vazia usa a URL oficial.",
        since: "0.7.0",
    },
    SettingSpec {
        key: EMBEDDING_API_KEY,
        kind: SettingType::Secret,
        default: "null",
        channel_override: false,
        description: "Chave de API do provedor de embedding, cifrada com `SECRETS_KEY`. Grave com `set_secret`; obrigatória no `gemini`. Trocar a chave não reindexa.",
        since: "0.7.0",
    },
    SettingSpec {
        key: EMBEDDING_DIMENSIONS,
        kind: SettingType::Integer { min: 1, max: 8192 },
        default: "null",
        channel_override: false,
        description: "Dimensão pedida ao provedor (`dimensions` no OpenAI-compatível, `outputDimensionality` no Gemini). Vazia, vale a do modelo. Entra no `model_id` e reindexa.",
        since: "0.7.0",
    },
    SettingSpec {
        key: EMBEDDING_QUERY_PREFIX,
        kind: SettingType::Text { max: 200 },
        default: "null",
        channel_override: false,
        description: "Prefixo das consultas. Vazio, vale o automático: `query: ` na família e5, nada nos outros modelos.",
        since: "0.7.0",
    },
    SettingSpec {
        key: EMBEDDING_DOCUMENT_PREFIX,
        kind: SettingType::Text { max: 200 },
        default: "null",
        channel_override: false,
        description: "Prefixo das memórias. Vazio, vale o automático: `passage: ` na família e5, nada nos outros modelos.",
        since: "0.7.0",
    },
    SettingSpec {
        key: EMBEDDING_BATCH_SIZE,
        kind: SettingType::Integer { min: 1, max: 256 },
        default: "32",
        channel_override: false,
        description: "Memórias por chamada ao provedor nos jobs da reindexação.",
        since: "0.7.0",
    },
    SettingSpec {
        key: EMBEDDING_TIMEOUT_MS,
        kind: SettingType::Integer {
            min: 100,
            max: 120_000,
        },
        default: "15000",
        channel_override: false,
        description: "Tempo limite de cada chamada de embedding dos jobs e testes, em milissegundos. A consulta da busca e das dicas espera no máximo 2 s.",
        since: "0.7.0",
    },
    SettingSpec {
        key: SEARCH_VECTOR_WEIGHT,
        kind: SettingType::Number {
            min: 0.0,
            max: 10.0,
        },
        default: "1.0",
        channel_override: false,
        description: "Peso do ramo vetorial na fusão RRF de `search_memories`.",
        since: "0.7.0",
    },
    SettingSpec {
        key: SEARCH_TEXT_WEIGHT,
        kind: SettingType::Number {
            min: 0.0,
            max: 10.0,
        },
        default: "1.0",
        channel_override: false,
        description: "Peso do ramo textual (FTS5/BM25) na fusão RRF de `search_memories`.",
        since: "0.7.0",
    },
    SettingSpec {
        key: SEARCH_RRF_K,
        kind: SettingType::Integer { min: 1, max: 1000 },
        default: "60",
        channel_override: false,
        description: "Constante `k` da fusão RRF: `score = Σ peso / (k + posição)`.",
        since: "0.7.0",
    },
    SettingSpec {
        key: SEARCH_DEFAULT_LIMIT,
        kind: SettingType::Integer { min: 1, max: 50 },
        default: "10",
        channel_override: false,
        description: "Resultados de `search_memories` quando `limit` não é informado.",
        since: "0.7.0",
    },
    SettingSpec {
        key: SEARCH_HINT_THRESHOLD,
        kind: SettingType::Number { min: 0.0, max: 1.0 },
        default: "0.8",
        channel_override: false,
        description: "Similaridade de cosseno mínima de uma dica (`get_hints`). Modelos e5 comprimem os cossenos: valores perto de 0,88 filtram melhor.",
        since: "0.7.0",
    },
    SettingSpec {
        key: SEARCH_HINT_LIMIT,
        kind: SettingType::Integer { min: 1, max: 20 },
        default: "3",
        channel_override: false,
        description: "Dicas devolvidas por `get_hints` quando `limit` não é informado.",
        since: "0.7.0",
    },
    SettingSpec {
        key: WEBHOOKS_ENABLED,
        kind: SettingType::Boolean,
        default: "true",
        channel_override: false,
        description: "Liga os webhooks: desligado, as mutações não enfileiram eventos e as entregas pendentes terminam como `skipped`.",
        since: "0.8.0",
    },
    SettingSpec {
        key: WEBHOOKS_TIMEOUT_MS,
        kind: SettingType::Integer {
            min: 500,
            max: 30_000,
        },
        default: "5000",
        channel_override: false,
        description: "Prazo de cada tentativa de entrega de um webhook.",
        since: "0.8.0",
    },
    SettingSpec {
        key: WEBHOOKS_MAX_ATTEMPTS,
        kind: SettingType::Integer { min: 1, max: 10 },
        default: "5",
        channel_override: false,
        description: "Tentativas de entrega de um evento antes de o job ficar `failed`.",
        since: "0.8.0",
    },
    SettingSpec {
        key: RETENTION_USAGE_DAYS,
        kind: SettingType::Integer { min: 1, max: 3650 },
        default: "90",
        channel_override: false,
        description: "Dias de `usage_log` guardados (base de `get_usage`).",
        since: "0.8.0",
    },
    SettingSpec {
        key: RETENTION_LLM_CALLS_DAYS,
        kind: SettingType::Integer { min: 1, max: 3650 },
        default: "90",
        channel_override: false,
        description: "Dias de `llm_calls` guardados.",
        since: "0.8.0",
    },
    SettingSpec {
        key: RETENTION_AUDIT_DAYS,
        kind: SettingType::Integer { min: 1, max: 3650 },
        default: "null",
        channel_override: false,
        description: "Dias de `audit_log` guardados; vazio guarda para sempre.",
        since: "0.8.0",
    },
    SettingSpec {
        key: RETENTION_JOBS_DAYS,
        kind: SettingType::Integer { min: 1, max: 3650 },
        default: "30",
        channel_override: false,
        description: "Dias que jobs `done` e `failed` ficam na fila depois de terminar; `queued` e `running` nunca saem.",
        since: "0.8.0",
    },
];

pub fn spec(key: &str) -> Option<&'static SettingSpec> {
    SETTINGS.iter().find(|spec| spec.key == key)
}

fn default_value(spec: &SettingSpec) -> Value {
    serde_json::from_str(spec.default).expect("padrao do catalogo e JSON valido")
}

fn unknown(key: &str) -> ApiError {
    ApiError::new(
        "SETTING_UNKNOWN",
        format!("Configuracao desconhecida: {key}"),
    )
    .with_details(json!({
        "key": key,
        "valid": SETTINGS.iter().map(|spec| spec.key).collect::<Vec<_>>(),
    }))
}

/// Chave que aceita sobreposição por canal; as outras → `400 SETTING_UNKNOWN` com as válidas.
pub fn channel_spec(key: &str) -> Result<&'static SettingSpec, ApiError> {
    spec(key)
        .filter(|spec| spec.channel_override)
        .ok_or_else(|| {
            ApiError::new(
                "SETTING_UNKNOWN",
                format!("Configuracao sem sobreposicao por canal: {key}"),
            )
            .with_details(json!({
                "key": key,
                "valid": SETTINGS
                    .iter()
                    .filter(|spec| spec.channel_override)
                    .map(|spec| spec.key)
                    .collect::<Vec<_>>(),
            }))
        })
}

/// Categoria de memória (`categories.list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Category {
    /// Maiúsculas, dígitos e `_`, começando por letra (até 40 caracteres).
    #[schema(example = "CONSTRAINTS")]
    pub key: String,
    #[schema(example = "Restrições")]
    pub label: String,
    /// De -1000 a 1000; maior entra antes no contexto.
    #[schema(example = 30)]
    pub priority: i64,
}

fn valid_category_key(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= MAX_CATEGORY_KEY
        && chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn parse_categories(value: &Value) -> Result<Vec<Category>, String> {
    let categories: Vec<Category> = serde_json::from_value(value.clone())
        .map_err(|_| "uma lista de { key, label, priority }".to_owned())?;
    if categories.is_empty() || categories.len() > MAX_CATEGORIES {
        return Err(format!("de 1 a {MAX_CATEGORIES} categorias"));
    }
    let mut keys = BTreeSet::new();
    for category in &categories {
        if !valid_category_key(&category.key) {
            return Err(format!(
                "key {:?} deve ter maiusculas, digitos e _, comecando por letra (ate {MAX_CATEGORY_KEY})",
                category.key
            ));
        }
        if !keys.insert(category.key.as_str()) {
            return Err(format!("key repetida: {}", category.key));
        }
        let label = category.label.trim().chars().count();
        if label == 0 || label > MAX_CATEGORY_LABEL {
            return Err(format!("label de 1 a {MAX_CATEGORY_LABEL} caracteres"));
        }
        if !(-1000..=1000).contains(&category.priority) {
            return Err("priority de -1000 a 1000".to_owned());
        }
    }
    Ok(categories)
}

/// Confere o valor contra o tipo e o intervalo da chave.
pub fn validate(spec: &SettingSpec, value: &Value) -> Result<(), ApiError> {
    let invalid = |message: String, details: Value| {
        Err(ApiError::new("SETTING_INVALID", message).with_details(details))
    };
    if value.is_null() && spec.default == "null" {
        return Ok(());
    }
    match spec.kind {
        SettingType::Integer { min, max } => match value.as_i64() {
            Some(number) if (min..=max).contains(&number) => Ok(()),
            _ => invalid(
                format!("{} deve ser um inteiro de {min} a {max}", spec.key),
                json!({ "key": spec.key, "type": "integer", "min": min, "max": max }),
            ),
        },
        SettingType::Number { min, max } => match value.as_f64() {
            Some(number) if (min..=max).contains(&number) => Ok(()),
            _ => invalid(
                format!("{} deve ser um numero de {min} a {max}", spec.key),
                json!({ "key": spec.key, "type": "number", "min": min, "max": max }),
            ),
        },
        SettingType::Boolean => match value {
            Value::Bool(_) => Ok(()),
            _ => invalid(
                format!("{} deve ser true ou false", spec.key),
                json!({ "key": spec.key, "type": "boolean" }),
            ),
        },
        SettingType::Categories => match parse_categories(value) {
            Ok(_) => Ok(()),
            Err(reason) => invalid(
                format!("{} invalida: {reason}", spec.key),
                json!({ "key": spec.key, "type": "categories", "reason": reason }),
            ),
        },
        SettingType::Choice(choices) => match value.as_str() {
            Some(choice) if choices.contains(&choice) => Ok(()),
            _ => invalid(
                format!("{} deve ser um de: {}", spec.key, choices.join(", ")),
                json!({ "key": spec.key, "type": "enum", "choices": choices }),
            ),
        },
        SettingType::Text { max } => match value.as_str() {
            Some(text) if (1..=max).contains(&text.trim().chars().count()) => Ok(()),
            _ => invalid(
                format!("{} deve ser um texto de 1 a {max} caracteres", spec.key),
                json!({ "key": spec.key, "type": "text", "max": max }),
            ),
        },
        SettingType::Url => match value.as_str() {
            Some(url) if valid_url(url) => Ok(()),
            _ => invalid(
                format!("{} deve ser uma URL http:// ou https://", spec.key),
                json!({ "key": spec.key, "type": "url" }),
            ),
        },
        SettingType::TextList { max_items, max_len } => match value.as_array() {
            Some(items)
                if items.len() <= max_items
                    && items.iter().all(|item| {
                        item.as_str().is_some_and(|text| {
                            (1..=max_len).contains(&text.trim().chars().count())
                        })
                    }) =>
            {
                Ok(())
            }
            _ => invalid(
                format!(
                    "{} deve ser uma lista de ate {max_items} textos de 1 a {max_len} caracteres",
                    spec.key
                ),
                json!({ "key": spec.key, "type": "list", "max_items": max_items, "max_len": max_len }),
            ),
        },
        SettingType::Secret => Err(setting_is_secret(spec.key)),
    }
}

fn valid_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|rest| {
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        !host.is_empty()
            && url.len() <= 2000
            && !url.chars().any(|c| c.is_whitespace() || c.is_control())
    })
}

/// Segredos não passam por `update_settings` (`400 SETTING_IS_SECRET`).
pub fn setting_is_secret(key: &str) -> ApiError {
    ApiError::new(
        "SETTING_IS_SECRET",
        format!("{key} e um segredo: grave com set_secret"),
    )
    .with_details(json!({ "key": key, "operation": "set_secret" }))
}

/// Valores efetivos em memória (padrões sobrepostos pelos persistidos).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsSnapshot {
    values: BTreeMap<&'static str, Value>,
}

impl Default for SettingsSnapshot {
    fn default() -> Self {
        Self::from_rows(&[])
    }
}

impl SettingsSnapshot {
    pub fn from_rows(rows: &[SettingRow]) -> Self {
        let mut values: BTreeMap<&'static str, Value> = SETTINGS
            .iter()
            .map(|spec| (spec.key, default_value(spec)))
            .collect();
        for row in rows {
            if let Some(spec) = spec(&row.key)
                && let Ok(value) = serde_json::from_str::<Value>(&row.value_json)
                && validate(spec, &value).is_ok()
            {
                values.insert(spec.key, value);
            }
        }
        Self { values }
    }

    fn integer(&self, key: &str) -> i64 {
        self.values
            .get(key)
            .and_then(Value::as_i64)
            .or_else(|| spec(key).and_then(|spec| default_value(spec).as_i64()))
            .unwrap_or_default()
    }

    pub fn rate_per_minute(&self) -> u32 {
        u32::try_from(self.integer(RATE_PER_MINUTE)).unwrap_or(u32::MAX)
    }

    pub fn max_batch(&self) -> u32 {
        u32::try_from(self.integer(MAX_BATCH)).unwrap_or(u32::MAX)
    }

    /// Valor efetivo de uma chave do catálogo.
    pub fn value(&self, key: &str) -> Value {
        self.values
            .get(key)
            .cloned()
            .or_else(|| spec(key).map(default_value))
            .unwrap_or(Value::Null)
    }

    pub fn categories(&self) -> Vec<Category> {
        self.values
            .get(CATEGORIES)
            .and_then(|value| parse_categories(value).ok())
            .or_else(|| {
                spec(CATEGORIES).and_then(|spec| parse_categories(&default_value(spec)).ok())
            })
            .unwrap_or_default()
    }

    pub fn text_min(&self) -> usize {
        usize::try_from(self.integer(TEXT_MIN)).unwrap_or(usize::MAX)
    }

    /// `memory.text_max` com a sobreposição do canal, quando houver uma válida.
    pub fn text_max(&self, overrides: Option<&BTreeMap<String, Value>>) -> usize {
        let value = overrides
            .and_then(|overrides| overrides.get(TEXT_MAX))
            .filter(|value| spec(TEXT_MAX).is_some_and(|spec| validate(spec, value).is_ok()))
            .and_then(Value::as_i64)
            .unwrap_or_else(|| self.integer(TEXT_MAX));
        usize::try_from(value).unwrap_or(usize::MAX)
    }

    pub fn dedupe_exact(&self) -> bool {
        self.values
            .get(DEDUPE_EXACT)
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    pub fn similar_threshold(&self) -> f64 {
        self.values
            .get(SIMILAR_THRESHOLD)
            .and_then(Value::as_f64)
            .unwrap_or(0.85)
    }

    fn text(&self, key: &str) -> String {
        self.value(key).as_str().unwrap_or_default().to_owned()
    }

    /// Orçamento global de uma faixa do contexto.
    pub fn budget(&self, key: &str) -> u32 {
        u32::try_from(self.integer(key)).unwrap_or(u32::MAX)
    }

    /// Teto de uma faixa no canal; `None` sem sobreposição válida.
    pub fn channel_budget(key: &str, overrides: Option<&BTreeMap<String, Value>>) -> Option<u32> {
        overrides
            .and_then(|overrides| overrides.get(key))
            .filter(|value| spec(key).is_some_and(|spec| validate(spec, value).is_ok()))
            .and_then(Value::as_u64)
            .map(|value| u32::try_from(value).unwrap_or(u32::MAX))
    }

    pub fn default_format(&self) -> String {
        self.text(DEFAULT_FORMAT)
    }

    pub fn token_estimator(&self) -> String {
        self.text(TOKEN_ESTIMATOR)
    }

    /// Booleano efetivo (padrão do catálogo quando ausente).
    pub fn boolean(&self, key: &str) -> bool {
        self.value(key).as_bool().unwrap_or_default()
    }

    pub fn number(&self, key: &str) -> f64 {
        self.value(key).as_f64().unwrap_or_default()
    }

    pub fn whole(&self, key: &str) -> i64 {
        self.integer(key)
    }

    /// Inteiro efetivo que aceita vazio (`null`).
    pub fn optional_whole(&self, key: &str) -> Option<i64> {
        self.value(key).as_i64()
    }

    /// Texto efetivo sem espaços nas pontas; `None` quando vazio ou nulo.
    pub fn optional_text(&self, key: &str) -> Option<String> {
        self.value(key)
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    }

    pub fn text_list(&self, key: &str) -> Vec<String> {
        self.value(key)
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Valor no canal: a sobreposição válida, senão o global.
    pub fn in_channel(&self, key: &str, overrides: Option<&BTreeMap<String, Value>>) -> Value {
        overrides
            .and_then(|overrides| overrides.get(key))
            .filter(|value| spec(key).is_some_and(|spec| validate(spec, value).is_ok()))
            .cloned()
            .unwrap_or_else(|| self.value(key))
    }

    /// Cópia com os valores propostos (já validados), sem gravar nada (`test_redator`).
    pub fn with(&self, proposed: &BTreeMap<&'static str, Value>) -> Self {
        let mut values = self.values.clone();
        for (key, value) in proposed {
            match spec(key) {
                Some(spec) if value.is_null() => {
                    values.insert(spec.key, default_value(spec));
                }
                Some(spec) => {
                    values.insert(spec.key, value.clone());
                }
                None => {}
            }
        }
        Self { values }
    }

    pub fn overflow_line(&self) -> bool {
        self.values
            .get(OVERFLOW_LINE)
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }
}

/// Uma configuração com sobreposição por canal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelSettingView {
    #[schema(example = "memory.text_max")]
    pub key: String,
    /// Valor efetivo no canal.
    #[schema(value_type = Object, example = 2000)]
    pub value: Value,
    /// Valor global (`get_settings`).
    #[schema(value_type = Object, example = 4000)]
    pub global: Value,
    /// Sobreposição do canal; `null` quando vale o global.
    #[serde(rename = "override")]
    #[schema(value_type = Option<Object>, example = 2000)]
    pub channel_value: Option<Value>,
    #[serde(rename = "type")]
    #[schema(example = "integer")]
    pub kind: String,
    #[schema(value_type = Option<f64>, example = 10)]
    pub min: Option<serde_json::Number>,
    #[schema(value_type = Option<f64>, example = 20000)]
    pub max: Option<serde_json::Number>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelSettingsView {
    #[schema(example = "eng/global")]
    pub channel: String,
    #[schema(example = "ch_1a2b3c4d")]
    pub channel_id: String,
    /// As chaves que aceitam sobreposição por canal.
    pub settings: Vec<ChannelSettingView>,
}

/// Mapa parcial `chave → valor` de sobreposições do canal; `null` remove a sobreposição.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[schema(example = json!({ "memory.text_max": 2000 }))]
pub struct UpdateChannelSettingsBody(pub BTreeMap<String, Value>);

pub fn channel_view(
    snapshot: &SettingsSnapshot,
    channel: &Channel,
    overrides: Option<&BTreeMap<String, Value>>,
) -> ChannelSettingsView {
    let settings = SETTINGS
        .iter()
        .filter(|spec| spec.channel_override)
        .map(|spec| {
            let global = snapshot.value(spec.key);
            let channel_value = overrides
                .and_then(|overrides| overrides.get(spec.key))
                .filter(|value| validate(spec, value).is_ok())
                .cloned();
            let (min, max) = spec.kind.bounds();
            ChannelSettingView {
                key: spec.key.to_owned(),
                value: channel_value.clone().unwrap_or_else(|| global.clone()),
                global,
                channel_value,
                kind: spec.kind.name().to_owned(),
                min,
                max,
                description: spec.description.to_owned(),
            }
        })
        .collect();
    ChannelSettingsView {
        channel: channel.slug.clone(),
        channel_id: channel.id.clone(),
        settings,
    }
}

/// Uma configuração com o valor efetivo, o padrão e a regra.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SettingView {
    #[schema(example = "limits.rate_per_minute")]
    pub key: String,
    #[schema(value_type = Object, example = 600)]
    pub value: Value,
    #[schema(value_type = Object, example = 600)]
    pub default: Value,
    #[serde(rename = "type")]
    #[schema(example = "integer")]
    pub kind: String,
    /// Mínimo (`integer`, `number`) ou itens mínimos (`categories`); `null` para `boolean`.
    #[schema(value_type = Option<f64>, example = 1)]
    pub min: Option<serde_json::Number>,
    #[schema(value_type = Option<f64>, example = 100000)]
    pub max: Option<serde_json::Number>,
    /// Valores aceitos (tipo `enum`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(example = json!(["tagged", "markdown", "json"]))]
    pub choices: Option<Vec<String>>,
    /// Se aceita sobreposição por canal (a partir da 003).
    pub channel_override: bool,
    pub description: String,
    /// `null` quando vale o padrão (no segredo, a última gravação).
    pub updated_at: Option<String>,
    pub updated_by_app: Option<String>,
    /// Chave de segredo: `value` e `default` são sempre `null`; grave com `set_secret`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secret: bool,
    /// Só nos segredos: se há valor gravado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SettingsView {
    pub settings: Vec<SettingView>,
}

#[derive(Debug, Clone, Default, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct GetSettingsQuery {
    /// Uma chave só; omita para todas.
    #[param(example = "limits.rate_per_minute")]
    pub key: Option<String>,
}

/// Mapa parcial `chave → valor`; `null` volta ao padrão.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[schema(example = json!({ "limits.rate_per_minute": 1200 }))]
pub struct UpdateSettingsBody(pub BTreeMap<String, Value>);

pub fn view(
    rows: &[SettingRow],
    secrets: &[SecretView],
    key: Option<&str>,
) -> Result<SettingsView, ApiError> {
    if let Some(key) = key
        && spec(key).is_none()
    {
        return Err(unknown(key));
    }
    let settings = SETTINGS
        .iter()
        .filter(|spec| key.is_none_or(|key| key == spec.key))
        .map(|spec| {
            if spec.kind == SettingType::Secret {
                let stored = secrets.iter().find(|secret| secret.key == spec.key);
                return SettingView {
                    key: spec.key.to_owned(),
                    value: Value::Null,
                    default: Value::Null,
                    kind: spec.kind.name().to_owned(),
                    min: None,
                    max: None,
                    choices: None,
                    channel_override: false,
                    description: spec.description.to_owned(),
                    updated_at: stored.and_then(|secret| secret.updated_at.clone()),
                    updated_by_app: None,
                    secret: true,
                    configured: Some(stored.is_some_and(|secret| secret.configured)),
                };
            }
            let row = rows.iter().find(|row| row.key == spec.key);
            let default = default_value(spec);
            let value = row
                .and_then(|row| serde_json::from_str(&row.value_json).ok())
                .unwrap_or_else(|| default.clone());
            let (min, max) = spec.kind.bounds();
            SettingView {
                key: spec.key.to_owned(),
                value,
                default,
                kind: spec.kind.name().to_owned(),
                min,
                max,
                choices: match spec.kind {
                    SettingType::Choice(choices) => {
                        Some(choices.iter().map(|choice| (*choice).to_owned()).collect())
                    }
                    _ => None,
                },
                channel_override: spec.channel_override,
                description: spec.description.to_owned(),
                updated_at: row.map(|row| row.updated_at.clone()),
                updated_by_app: row.and_then(|row| row.updated_by_app.clone()),
                secret: false,
                configured: None,
            }
        })
        .collect();
    Ok(SettingsView { settings })
}

impl<S: Store> Core<S> {
    /// `409 CATEGORY_IN_USE` com a contagem das categorias usadas fora de `retained`.
    async fn category_in_use(&self, retained: Vec<String>) -> ApiError {
        let counts = match self.store().category_counts().await {
            Ok(counts) => counts,
            Err(error) => return ApiError::from(error),
        };
        let in_use: BTreeMap<String, u64> = counts
            .into_iter()
            .filter(|(key, _)| !retained.contains(key))
            .collect();
        ApiError::new(
            "CATEGORY_IN_USE",
            "Ha memorias nas categorias retiradas; mova ou recategorize antes",
        )
        .with_details(json!({ "in_use": in_use }))
    }

    pub async fn get_settings(
        &self,
        principal: &Principal,
        query: GetSettingsQuery,
    ) -> Result<SettingsView, ApiError> {
        self.authorize(principal, "get_settings")?;
        let rows = self.store().get_settings().await?;
        let secrets = self.store().list_secrets().await?;
        view(&rows, &secrets, query.key.as_deref().map(str::trim))
    }

    pub async fn update_settings(
        &self,
        principal: &Principal,
        body: UpdateSettingsBody,
        key: Option<&str>,
    ) -> Result<Mutation<SettingsView>, ApiError> {
        let operation = self.authorize(principal, "update_settings")?;
        if body.0.is_empty() {
            return Err(ApiError::validation("Informe ao menos uma configuracao"));
        }
        let mut changes = Vec::with_capacity(body.0.len());
        let mut retained = None;
        for (name, value) in &body.0 {
            let spec = spec(name).ok_or_else(|| unknown(name))?;
            if spec.kind == SettingType::Secret {
                return Err(setting_is_secret(spec.key));
            }
            let value_json = if value.is_null() {
                None
            } else {
                validate(spec, value)?;
                Some(value.to_string())
            };
            if spec.key == CATEGORIES {
                let effective = if value.is_null() {
                    default_value(spec)
                } else {
                    value.clone()
                };
                retained = parse_categories(&effective)
                    .ok()
                    .map(|categories| categories.into_iter().map(|c| c.key).collect::<Vec<_>>());
            }
            changes.push(SettingChange {
                key: spec.key.to_owned(),
                value_json,
            });
        }
        let current = self.settings_snapshot();
        let proposed: BTreeMap<&'static str, Value> = body
            .0
            .iter()
            .filter_map(|(name, value)| spec(name).map(|spec| (spec.key, value.clone())))
            .collect();
        // Troca do espaço dos vetores: o provedor é validado antes da transação (007 FR-008).
        let switch = self
            .embedding_switch(&current, &current.with(&proposed))
            .await?;
        let core = self.clone();
        let stored_switch = switch.clone();
        let keys: Vec<String> = changes.iter().map(|change| change.key.clone()).collect();
        let mutation = self
            .mutate(
                principal,
                operation,
                key,
                &body,
                200,
                |mut context| async move {
                    context.events = core.settings_event(None, &keys).await?;
                    match core
                        .store()
                        .update_settings(changes, retained.clone(), stored_switch, context)
                        .await
                    {
                        Err(StoreError::Conflict("CATEGORY_IN_USE")) => {
                            Err(core.category_in_use(retained.unwrap_or_default()).await)
                        }
                        other => {
                            other.map_err(|error| super::store_error(error, ApiError::internal))
                        }
                    }
                },
            )
            .await?;
        self.replace_settings(SettingsSnapshot::from_rows(&mutation.value));
        if let Some(switch) = &switch
            && !mutation.replayed
        {
            self.embedding_switched(switch).await;
        }
        let secrets = self.store().list_secrets().await?;
        Ok(Mutation {
            value: view(&mutation.value, &secrets, None)?,
            replayed: mutation.replayed,
        })
    }
}

impl<S: Store> Core<S> {
    pub async fn get_channel_settings(
        &self,
        principal: &Principal,
        channel: &str,
    ) -> Result<ChannelSettingsView, ApiError> {
        self.authorize(principal, "get_channel_settings")?;
        let network = self.network().await?;
        let channel = super::channels::find_in(&network, channel)?;
        Ok(channel_view(
            &self.settings_snapshot(),
            channel,
            network.channel_settings.get(&channel.id),
        ))
    }

    pub async fn update_channel_settings(
        &self,
        principal: &Principal,
        channel: &str,
        body: UpdateChannelSettingsBody,
        key: Option<&str>,
    ) -> Result<Mutation<ChannelSettingsView>, ApiError> {
        let operation = self.authorize(principal, "update_channel_settings")?;
        if body.0.is_empty() {
            return Err(ApiError::validation("Informe ao menos uma configuracao"));
        }
        let mut changes = Vec::with_capacity(body.0.len());
        for (name, value) in &body.0 {
            let spec = channel_spec(name)?;
            let value_json = if value.is_null() {
                None
            } else {
                validate(spec, value)?;
                Some(value.to_string())
            };
            changes.push(SettingChange {
                key: spec.key.to_owned(),
                value_json,
            });
        }
        super::channels::channel_lookup(channel)?;
        let input = json!({ "channel": channel, "settings": body });
        let core = self.clone();
        let target = channel.to_owned();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |mut context| async move {
                    let network = core.network().await?;
                    let found = super::channels::find_in(&network, &target)?;
                    let keys: Vec<String> =
                        changes.iter().map(|change| change.key.clone()).collect();
                    context.events = core.settings_event(Some(&found.slug), &keys).await?;
                    let id = found.id.clone();
                    core.store()
                        .update_channel_settings(id, changes, context)
                        .await
                        .map_err(|error| {
                            super::store_error(error, super::channels::channel_not_found)
                        })
                },
            )
            .await;
        self.network_changed();
        let mutation = result?;
        let network = self.network().await?;
        let channel = super::channels::find_in(&network, channel)?;
        Ok(Mutation {
            value: channel_view(&self.settings_snapshot(), channel, Some(&mutation.value)),
            replayed: mutation.replayed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_validated_against_the_range() {
        let rate = spec(RATE_PER_MINUTE).expect("rate");
        assert!(validate(rate, &json!(1)).is_ok());
        assert!(validate(rate, &json!(100_000)).is_ok());
        for invalid in [
            json!(0),
            json!(100_001),
            json!(1.5),
            json!("600"),
            json!(-1),
        ] {
            assert_eq!(
                validate(rate, &invalid).expect_err("invalid").code,
                "SETTING_INVALID",
                "{invalid}"
            );
        }
    }

    #[test]
    fn snapshot_uses_defaults_and_ignores_invalid_rows() {
        let snapshot = SettingsSnapshot::default();
        assert_eq!(snapshot.rate_per_minute(), 600);
        assert_eq!(snapshot.max_batch(), 100);
        let row = |key: &str, value: &str| SettingRow {
            key: key.to_owned(),
            value_json: value.to_owned(),
            updated_by_app: None,
            updated_at: "2026-10-30T12:00:00.000Z".to_owned(),
        };
        let snapshot =
            SettingsSnapshot::from_rows(&[row(RATE_PER_MINUTE, "5"), row(MAX_BATCH, "\"x\"")]);
        assert_eq!(snapshot.rate_per_minute(), 5);
        assert_eq!(snapshot.max_batch(), 100);
    }

    #[test]
    fn every_default_is_valid() {
        for spec in SETTINGS {
            validate(spec, &default_value(spec)).expect(spec.key);
        }
    }

    #[test]
    fn numbers_booleans_and_categories_are_validated() {
        let threshold = spec(SIMILAR_THRESHOLD).expect("threshold");
        assert!(validate(threshold, &json!(0)).is_ok());
        assert!(validate(threshold, &json!(0.5)).is_ok());
        assert!(validate(threshold, &json!(1.2)).is_err());
        assert!(validate(threshold, &json!("0.5")).is_err());
        let dedupe = spec(DEDUPE_EXACT).expect("dedupe");
        assert!(validate(dedupe, &json!(false)).is_ok());
        assert!(validate(dedupe, &json!(0)).is_err());
        let categories = spec(CATEGORIES).expect("categories");
        let one = json!([{ "key": "RULES", "label": "Regras", "priority": 1 }]);
        assert!(validate(categories, &one).is_ok());
        for invalid in [
            json!([]),
            json!([{ "key": "rules", "label": "R", "priority": 1 }]),
            json!([{ "key": "A", "label": "", "priority": 1 }]),
            json!([{ "key": "A", "label": "A", "priority": 2000 }]),
            json!([
                { "key": "A", "label": "A", "priority": 1 },
                { "key": "A", "label": "B", "priority": 2 }
            ]),
            json!([{ "key": "A", "label": "A", "priority": 1, "x": 1 }]),
        ] {
            assert_eq!(
                validate(categories, &invalid).expect_err("invalid").code,
                "SETTING_INVALID",
                "{invalid}"
            );
        }
    }

    #[test]
    fn memory_settings_have_typed_readers_and_channel_overrides() {
        let snapshot = SettingsSnapshot::default();
        assert_eq!(snapshot.text_min(), 3);
        assert_eq!(snapshot.text_max(None), 4000);
        assert!(snapshot.dedupe_exact());
        assert!((snapshot.similar_threshold() - 0.85).abs() < 1e-9);
        assert_eq!(snapshot.categories().len(), 5);
        assert_eq!(snapshot.categories()[0].key, "PROJECT_RULES");
        let overrides = BTreeMap::from([(TEXT_MAX.to_owned(), json!(200))]);
        assert_eq!(snapshot.text_max(Some(&overrides)), 200);
        let invalid = BTreeMap::from([(TEXT_MAX.to_owned(), json!(1))]);
        assert_eq!(snapshot.text_max(Some(&invalid)), 4000);
        assert!(channel_spec(TEXT_MAX).is_ok());
        let error = channel_spec(TEXT_MIN).expect_err("global");
        assert_eq!(error.code, "SETTING_UNKNOWN");
        assert_eq!(
            error.details.expect("details")["valid"],
            json!([
                TEXT_MAX,
                BUDGET_TOKENS,
                TITLES_BUDGET_TOKENS,
                REDATOR_INSTRUCTIONS,
                REDATOR_REWRITE_TEXT
            ])
        );
    }

    #[test]
    fn context_settings_have_typed_readers_and_choices() {
        let snapshot = SettingsSnapshot::default();
        assert_eq!(snapshot.budget(BUDGET_TOKENS), 4000);
        assert_eq!(snapshot.budget(TITLES_BUDGET_TOKENS), 800);
        assert_eq!(snapshot.default_format(), "tagged");
        assert_eq!(snapshot.token_estimator(), "o200k");
        assert!(snapshot.overflow_line());
        let overrides = BTreeMap::from([(BUDGET_TOKENS.to_owned(), json!(100))]);
        assert_eq!(
            SettingsSnapshot::channel_budget(BUDGET_TOKENS, Some(&overrides)),
            Some(100)
        );
        assert_eq!(
            SettingsSnapshot::channel_budget(TITLES_BUDGET_TOKENS, Some(&overrides)),
            None
        );
        let format = spec(DEFAULT_FORMAT).expect("format");
        assert!(validate(format, &json!("json")).is_ok());
        let error = validate(format, &json!("xml")).expect_err("invalid");
        assert_eq!(error.code, "SETTING_INVALID");
        assert_eq!(
            error.details.expect("details")["choices"],
            json!(["tagged", "markdown", "json"])
        );
        assert_eq!(format.kind.range(), "`tagged`, `markdown`, `json`");
        let view = view(&[], &[], Some(DEFAULT_FORMAT)).expect("view");
        assert_eq!(view.settings[0].kind, "enum");
        assert_eq!(
            view.settings[0].choices.as_deref(),
            Some(
                &[
                    "tagged".to_owned(),
                    "markdown".to_owned(),
                    "json".to_owned()
                ][..]
            )
        );
    }

    #[test]
    fn redator_settings_validate_text_urls_lists_and_secrets() {
        let model = spec(REDATOR_MODEL).expect("model");
        assert!(validate(model, &json!("gpt-4o-mini")).is_ok());
        assert!(validate(model, &Value::Null).is_ok());
        assert!(validate(model, &json!("")).is_err());
        assert!(validate(model, &json!("x".repeat(201))).is_err());
        let url = spec(REDATOR_BASE_URL).expect("url");
        assert!(validate(url, &json!("https://api.openai.com/v1")).is_ok());
        assert!(validate(url, &json!("http://127.0.0.1:11434/v1")).is_ok());
        for invalid in [
            json!("ftp://x"),
            json!("https://"),
            json!("https://a b"),
            json!(1),
        ] {
            assert!(validate(url, &invalid).is_err(), "{invalid}");
        }
        let fallback = spec(REDATOR_FALLBACK_MODELS).expect("fallback");
        assert!(validate(fallback, &json!([])).is_ok());
        assert!(validate(fallback, &json!(["a", "b"])).is_ok());
        assert!(validate(fallback, &json!(["a", ""])).is_err());
        assert!(validate(fallback, &json!(vec!["a"; 6])).is_err());
        let provider = spec(REDATOR_PROVIDER).expect("provider");
        assert!(validate(provider, &json!("gemini")).is_ok());
        assert!(validate(provider, &json!("ollama")).is_err());
        let secret = spec(REDATOR_API_KEY).expect("secret");
        assert_eq!(
            validate(secret, &json!("sk")).expect_err("secret").code,
            "SETTING_IS_SECRET"
        );
        let snapshot = SettingsSnapshot::default();
        assert!(snapshot.boolean(REDATOR_ENABLED));
        assert_eq!(snapshot.optional_text(REDATOR_MODEL), None);
        assert_eq!(snapshot.whole(REDATOR_TITLE_MAX), 100);
        assert!(snapshot.text_list(REDATOR_FALLBACK_MODELS).is_empty());
        let overrides = BTreeMap::from([(REDATOR_REWRITE_TEXT.to_owned(), json!(false))]);
        assert_eq!(
            snapshot.in_channel(REDATOR_REWRITE_TEXT, Some(&overrides)),
            json!(false)
        );
        let proposed = BTreeMap::from([(REDATOR_MODEL, json!("m"))]);
        assert_eq!(
            snapshot
                .with(&proposed)
                .optional_text(REDATOR_MODEL)
                .as_deref(),
            Some("m")
        );
    }

    #[test]
    fn secrets_are_listed_without_value() {
        let secrets = [SecretView {
            key: REDATOR_API_KEY.to_owned(),
            configured: true,
            updated_at: Some("2026-10-31T12:00:00.000Z".to_owned()),
        }];
        let view = view(&[], &secrets, Some(REDATOR_API_KEY)).expect("view");
        let secret = &view.settings[0];
        assert!(secret.secret);
        assert_eq!(secret.configured, Some(true));
        assert_eq!(secret.value, Value::Null);
        assert_eq!(secret.kind, "secret");
        let empty = super::view(&[], &[], Some(REDATOR_API_KEY)).expect("view");
        assert_eq!(empty.settings[0].configured, Some(false));
    }

    #[test]
    fn view_filters_by_key_and_rejects_unknown_keys() {
        let view_all = view(&[], &[], None).expect("all");
        assert_eq!(view_all.settings.len(), SETTINGS.len());
        let one = view(&[], &[], Some(MAX_BATCH)).expect("one");
        assert_eq!(one.settings.len(), 1);
        assert_eq!(
            view(&[], &[], Some("limits.x")).expect_err("unknown").code,
            "SETTING_UNKNOWN"
        );
    }
}
