//! `get_context` (desenho §8): o bloco de memórias pronto para o prompt, determinístico, com
//! `version`/ETag, `304` sem tocar o banco e cache LRU. A escolha das memórias é a mesma em
//! todos os formatos (005 Q-004); só a renderização muda.

pub mod budget;
pub mod cache;
pub mod estimator;
pub mod focus;
pub mod order;
pub mod render;
pub mod version;

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::{IntoParams, ToSchema};

use self::{
    budget::{Band, Limit, Line},
    estimator::{Tiktoken, TokenEstimator},
    order::Entry,
    render::{Block, ChannelBlock, Group, Item},
    version::{Fingerprint, ReachedChannel},
};
use super::{
    Core,
    access::{Purpose, Reach},
    consumption::{ChannelConsumption, Consumption, slot},
    ids,
    model::{ContextMemory, UsageEntry},
    principal::{Principal, Scope},
    selection::Selection,
    settings::{self, Category, SettingsSnapshot},
};
use crate::{error::ApiError, store::Store};

/// Títulos mostrados pelo resource `engramind://channels/{slug}` antes da linha final.
pub const CHANNEL_RESOURCE_LIMIT: usize = 500;

/// Formato do bloco.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextFormat {
    /// `#id: texto` agrupado por canal e categoria em tags curtas (padrão).
    #[default]
    Tagged,
    /// Seções Markdown, sem ids.
    Markdown,
    /// Objeto com canais, categorias, títulos e excedente.
    Json,
}

impl ContextFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tagged => "tagged",
            Self::Markdown => "markdown",
            Self::Json => "json",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "tagged" => Some(Self::Tagged),
            "markdown" => Some(Self::Markdown),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    /// Tipo MIME do `content` (resource MCP).
    pub fn mime(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Tagged | Self::Markdown => "text/plain",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct GetContextQuery {
    /// Slugs e prefixos (`eng/global,eng/projeto-*`) separados por vírgula; restringe dentro do
    /// que o token alcança e do cabeçalho `Engramind-Channels`.
    pub channels: Option<String>,
    /// `tagged`, `markdown` ou `json`; padrão `context.default_format`.
    pub format: Option<ContextFormat>,
    /// Texto da tarefa ou da conversa (até 2000 caracteres depois de juntar os espaços): as
    /// memórias relevantes (FTS5 + vetor) vêm antes dentro de cada prioridade de canal. Entra na
    /// `version`; nunca é gravado em log nem em `usage_log`.
    pub focus: Option<String>,
}

/// Como o `focus` foi aplicado: `hybrid` (FTS5 + vetor), `text` (só FTS5: embedding
/// desligado, com falha ou fora do prazo) ou `none` (FTS5 também falhou: bloco sem foco).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FocusMode {
    Hybrid,
    Text,
    None,
}

impl FocusMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hybrid => "hybrid",
            Self::Text => "text",
            Self::None => "none",
        }
    }
}

/// Quantas memórias entraram em cada faixa.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContextCounts {
    /// Com texto completo (faixa 1).
    #[schema(example = 4)]
    pub full: u32,
    /// Só `#id: título` (faixa 2).
    #[schema(example = 2)]
    pub titles: u32,
    /// Fora das duas faixas (alcançáveis por `search_memories`).
    #[schema(example = 12)]
    pub overflow: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContextLine {
    #[schema(example = "m_8f2a1c3d")]
    pub id: String,
    /// Texto da memória, sem alteração; com `context.text_max_chars`, sem quebras e cortado
    /// com `…`.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContextCategory {
    #[schema(example = "CONSTRAINTS")]
    pub key: String,
    pub memories: Vec<ContextLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContextChannel {
    #[schema(example = "eng/global")]
    pub slug: String,
    pub categories: Vec<ContextCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContextTitle {
    #[schema(example = "m_77aa0100")]
    pub id: String,
    pub title: String,
    #[schema(example = "CONSTRAINTS")]
    pub category: String,
    /// Slug do canal da memória (distingue canais privados dos públicos no mesmo bloco).
    #[schema(example = "eng/global")]
    pub channel: String,
}

/// O bloco no formato `json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContextDocument {
    pub channels: Vec<ContextChannel>,
    pub titles: Vec<ContextTitle>,
    #[schema(example = 12)]
    pub overflow: u32,
}

/// Texto (`tagged`, `markdown`) ou objeto (`json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ContextContent {
    Text(String),
    Document(ContextDocument),
}

/// O contexto renderizado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContextView {
    pub content: ContextContent,
    /// Igual ao `ETag`; muda só com mudança real de memória, canal, assinatura ou
    /// configuração de renderização.
    #[schema(example = "cv_3f9e1a7c0b2d")]
    pub version: String,
    pub counts: ContextCounts,
    /// Tokens estimados do `content` (`context.token_estimator`).
    #[schema(example = 612)]
    pub rendered_tokens: u32,
    pub format: ContextFormat,
    /// Só com `focus`: como ele foi aplicado (também no cabeçalho `Engramind-Focus-Mode`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_mode: Option<FocusMode>,
}

/// Resposta do MCP e da CLI quando `if_none_match` é igual à `version` (o REST responde
/// `304` sem corpo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContextNotModified {
    pub not_modified: bool,
    #[schema(example = "cv_3f9e1a7c0b2d")]
    pub version: String,
}

/// Um bloco no cache, com os ids da faixa 1 e os canais alcançados.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub view: ContextView,
    pub full_ids: Vec<String>,
    pub channel_ids: Vec<String>,
    /// Faixa 1 e faixa 2 por canal, na ordem do bloco: o `memory.consumed` (decisão 0034).
    pub consumed: Vec<ChannelConsumption>,
}

impl Rendered {
    #[cfg(test)]
    fn empty(version: &str) -> Self {
        Self {
            view: ContextView {
                content: ContextContent::Text(String::new()),
                version: version.to_owned(),
                counts: ContextCounts::default(),
                rendered_tokens: 0,
                format: ContextFormat::Tagged,
                focus_mode: None,
            },
            full_ids: Vec::new(),
            channel_ids: Vec::new(),
            consumed: Vec::new(),
        }
    }
}

pub enum ContextOutcome {
    NotModified(ContextNotModified),
    Rendered(Arc<Rendered>),
}

/// Um canal alcançado como a renderização o usa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInput {
    pub id: String,
    pub slug: String,
    pub priority: i64,
    pub full_cap: Option<u32>,
    pub titles_cap: Option<u32>,
    /// Prioridade efetiva de cada categoria no canal (`categories.list` sobreposta por
    /// `context.category_priorities`).
    pub category_priorities: BTreeMap<String, i64>,
    /// `context.text_max_chars` no canal (0 = sem corte).
    pub text_max_chars: usize,
}

/// Tudo de que a renderização precisa; sem I/O.
#[derive(Debug, Clone)]
pub struct RenderInput {
    pub format: ContextFormat,
    pub version: String,
    pub channels: Vec<ChannelInput>,
    pub memories: Vec<ContextMemory>,
    pub categories: Vec<Category>,
    pub full_budget: u32,
    pub titles_budget: u32,
    pub overflow_line: bool,
    /// Posição de relevância das memórias relevantes ao `focus` (0 = a mais relevante).
    pub focus: Option<HashMap<String, usize>>,
    pub focus_mode: Option<FocusMode>,
}

/// Categorias de um canal: `(-prioridade, chave) → linhas`.
type Groups<'a> = BTreeMap<(i64, &'a str), Vec<Item<'a>>>;

/// Escolhe, ordena e renderiza (puro).
pub fn render(input: &RenderInput, estimator: &dyn TokenEstimator) -> Rendered {
    let channels: HashMap<&str, &ChannelInput> = input
        .channels
        .iter()
        .map(|channel| (channel.id.as_str(), channel))
        .collect();
    let category_priority: HashMap<&str, i64> = input
        .categories
        .iter()
        .map(|category| (category.key.as_str(), category.priority))
        .collect();
    let priority_of = |channel: &ChannelInput, category: &str| -> i64 {
        channel
            .category_priorities
            .get(category)
            .or_else(|| category_priority.get(category))
            .copied()
            .unwrap_or(i64::MIN)
    };
    let category_keys: Vec<String> = input
        .categories
        .iter()
        .map(|category| category.key.clone())
        .collect();
    let mut entries: Vec<Entry<'_>> = input
        .memories
        .iter()
        .filter_map(|memory| {
            let channel = channels.get(memory.channel_id.as_str())?;
            Some(Entry {
                memory,
                slug: channel.slug.as_str(),
                channel_priority: channel.priority,
                category_priority: priority_of(channel, &memory.category),
                focus_rank: input
                    .focus
                    .as_ref()
                    .and_then(|ranks| ranks.get(&memory.id).copied()),
            })
        })
        .collect();
    order::sort(&mut entries);
    // Faixa 1 com o corte de `context.text_max_chars` do canal; o orçamento mede a linha cortada.
    let full_texts: Vec<std::borrow::Cow<'_, str>> = entries
        .iter()
        .map(|entry| {
            let max = channels
                .get(entry.memory.channel_id.as_str())
                .map_or(0, |channel| channel.text_max_chars);
            if max == 0 {
                std::borrow::Cow::Borrowed(entry.memory.text.as_str())
            } else {
                std::borrow::Cow::Owned(render::preview(&entry.memory.text, max))
            }
        })
        .collect();
    let full_lines: Vec<String> = entries
        .iter()
        .zip(&full_texts)
        .map(|(entry, text)| {
            format!(
                "#{}: {}",
                entry.memory.id,
                render::escape_tagged(text, &category_keys)
            )
        })
        .collect();
    // Faixa 2 sem agrupamento: a categoria e o canal vão na linha (0.11.0).
    let title_lines: Vec<String> = entries
        .iter()
        .map(|entry| {
            format!(
                "#{} [{}] @{}: {}",
                entry.memory.id,
                entry.memory.category,
                entry.slug,
                render::escape_tagged(&entry.memory.title, &category_keys)
            )
        })
        .collect();
    let lines: Vec<Line<'_>> = entries
        .iter()
        .zip(full_lines.iter().zip(&title_lines))
        .map(|(entry, (full, title))| Line {
            channel: entry.memory.channel_id.as_str(),
            full,
            title,
        })
        .collect();
    let caps = |cap: fn(&ChannelInput) -> Option<u32>| -> BTreeMap<String, u32> {
        input
            .channels
            .iter()
            .filter_map(|channel| cap(channel).map(|value| (channel.id.clone(), value)))
            .collect()
    };
    let full_limit = Limit {
        global: input.full_budget,
        channels: caps(|channel| channel.full_cap),
    };
    let titles_limit = Limit {
        global: input.titles_budget,
        channels: caps(|channel| channel.titles_cap),
    };
    let bands = budget::place(&lines, &full_limit, &titles_limit, estimator);

    let mut counts = ContextCounts::default();
    let mut full_ids = Vec::new();
    let mut consumed: Vec<ChannelConsumption> = Vec::new();
    let mut grouped: BTreeMap<(i64, &str), Groups<'_>> = BTreeMap::new();
    let mut titles = Vec::new();
    for (((entry, band), (full, title)), text) in entries
        .iter()
        .zip(&bands)
        .zip(full_lines.into_iter().zip(title_lines))
        .zip(&full_texts)
    {
        match band {
            Band::Full => {
                counts.full += 1;
                full_ids.push(entry.memory.id.clone());
                slot(&mut consumed, entry.slug)
                    .memory_ids
                    .push(entry.memory.id.clone());
                grouped
                    .entry((-entry.channel_priority, entry.slug))
                    .or_default()
                    .entry((-entry.category_priority, entry.memory.category.as_str()))
                    .or_default()
                    .push(Item {
                        id: &entry.memory.id,
                        raw: text.as_ref(),
                        tagged: full,
                        category: &entry.memory.category,
                        channel: entry.slug,
                    });
            }
            Band::Title => {
                counts.titles += 1;
                slot(&mut consumed, entry.slug)
                    .title_ids
                    .push(entry.memory.id.clone());
                titles.push(Item {
                    id: &entry.memory.id,
                    raw: &entry.memory.title,
                    tagged: title,
                    category: &entry.memory.category,
                    channel: entry.slug,
                });
            }
            Band::Overflow => counts.overflow += 1,
        }
    }
    let block = Block {
        channels: grouped
            .into_iter()
            .map(|((_, slug), groups)| ChannelBlock {
                slug,
                groups: groups
                    .into_iter()
                    .map(|((_, key), items)| Group { key, items })
                    .collect(),
            })
            .collect(),
        titles,
        overflow: counts.overflow,
        overflow_line: input.overflow_line,
    };
    let (content, measured) = match input.format {
        ContextFormat::Tagged => {
            let text = render::tagged(&block, &input.version);
            let tokens = estimator.count(&text);
            (ContextContent::Text(text), tokens)
        }
        ContextFormat::Markdown => {
            let text = render::markdown(&block);
            let tokens = estimator.count(&text);
            (ContextContent::Text(text), tokens)
        }
        ContextFormat::Json => {
            let document = render::json(&block);
            let tokens = estimator.count(&serde_json::to_string(&document).unwrap_or_default());
            (ContextContent::Document(document), tokens)
        }
    };
    Rendered {
        view: ContextView {
            content,
            version: input.version.clone(),
            counts,
            rendered_tokens: measured,
            format: input.format,
            focus_mode: input.focus_mode,
        },
        full_ids,
        channel_ids: input
            .channels
            .iter()
            .map(|channel| channel.id.clone())
            .collect(),
        consumed,
    }
}

/// As configurações que mudam o bloco, como entram no hash da `version`.
fn render_settings(snapshot: &SettingsSnapshot) -> Value {
    Value::Object(
        settings::CONTEXT_RENDER_KEYS
            .iter()
            .map(|key| ((*key).to_owned(), snapshot.value(key)))
            .collect::<Map<_, _>>(),
    )
}

/// O foco já calculado para a renderização.
struct FocusRender {
    ranks: Option<HashMap<String, usize>>,
    mode: FocusMode,
    /// O modo esperado rodou: o bloco pode ir para o cache dos focos.
    cacheable: bool,
}

/// Acrescenta o foco às configurações que entram no hash da `version`.
fn with_focus(
    settings: &mut Value,
    focus: &str,
    model_id: &str,
    snapshot: &settings::SettingsSnapshot,
    mode: FocusMode,
) {
    if let Value::Object(map) = settings {
        map.insert(
            "focus".to_owned(),
            focus::fingerprint_entry(focus, model_id, snapshot, mode),
        );
    }
}

/// O que se sabe antes de ler as memórias: alcance, versão e sobreposições.
struct Plan {
    format: ContextFormat,
    channels: Vec<ChannelInput>,
    reached: Vec<(String, i64, &'static str, i64)>,
    fingerprint: Fingerprint,
    version: String,
}

fn fingerprint_of(
    format: ContextFormat,
    reached: &[(String, i64, &'static str, i64)],
    settings: &Value,
) -> Fingerprint {
    let channels: Vec<ReachedChannel<'_>> = reached
        .iter()
        .map(|(id, version, mode, priority)| ReachedChannel {
            id,
            version: *version,
            mode,
            priority: *priority,
        })
        .collect();
    version::fingerprint(format, &channels, settings)
}

impl<S: Store> Core<S> {
    /// Formato pedido ou `context.default_format`.
    pub fn context_format(&self, format: Option<ContextFormat>) -> ContextFormat {
        format.unwrap_or_else(|| {
            ContextFormat::parse(&self.settings_snapshot().default_format()).unwrap_or_default()
        })
    }

    /// Alcance e `version` a partir da rede em cache, sem tocar o banco (FR-009).
    async fn context_plan(
        &self,
        principal: &Principal,
        selection: &Selection,
        format: ContextFormat,
        settings: &Value,
    ) -> Result<Plan, ApiError> {
        let network = self.network().await?;
        let reaches: Vec<Reach> =
            super::access::resolve(&network, principal, selection, Purpose::Context, false);
        let channels = reaches
            .iter()
            .map(|reach| {
                let overrides = network.channel_settings.get(&reach.channel.id);
                ChannelInput {
                    id: reach.channel.id.clone(),
                    slug: reach.channel.slug.clone(),
                    priority: reach.priority,
                    full_cap: SettingsSnapshot::channel_budget(settings::BUDGET_TOKENS, overrides),
                    titles_cap: SettingsSnapshot::channel_budget(
                        settings::TITLES_BUDGET_TOKENS,
                        overrides,
                    ),
                    category_priorities: self.settings_snapshot().category_priorities(overrides),
                    text_max_chars: self.settings_snapshot().text_max_chars(overrides),
                }
            })
            .collect();
        let reached: Vec<(String, i64, &'static str, i64)> = reaches
            .iter()
            .map(|reach| {
                (
                    reach.channel.id.clone(),
                    reach.channel.version,
                    reach.mode.as_str(),
                    reach.priority,
                )
            })
            .collect();
        let fingerprint = fingerprint_of(format, &reached, settings);
        Ok(Plan {
            format,
            channels,
            reached,
            version: version::label(&fingerprint),
            fingerprint,
        })
    }

    /// O bloco do plano: do cache ou do banco. As memórias e as versões dos canais vêm da
    /// mesma leitura, e a `version` devolvida é a dessas versões (005 Q-010).
    async fn context_rendered(
        &self,
        plan: Plan,
        snapshot: &SettingsSnapshot,
        settings: &Value,
        focus: Option<FocusRender>,
    ) -> Result<Arc<Rendered>, ApiError> {
        // Com foco, o cache é o pequeno e separado; o bloco degradado não entra em cache.
        let cache = match &focus {
            None => Some(self.context_cache()),
            Some(focus) if focus.cacheable => Some(self.focus_cache()),
            Some(_) => None,
        };
        if let Some(hit) = cache.and_then(|cache| cache.get(&plan.fingerprint)) {
            return Ok(hit);
        }
        let read = self
            .store()
            .context_memories(
                plan.channels
                    .iter()
                    .map(|channel| channel.id.clone())
                    .collect(),
            )
            .await?;
        let versions: HashMap<&str, i64> = read
            .versions
            .iter()
            .map(|(id, version)| (id.as_str(), *version))
            .collect();
        let reached: Vec<(String, i64, &'static str, i64)> = plan
            .reached
            .iter()
            .filter_map(|(id, _, mode, priority)| {
                versions
                    .get(id.as_str())
                    .map(|version| (id.clone(), *version, *mode, *priority))
            })
            .collect();
        let fingerprint = fingerprint_of(plan.format, &reached, settings);
        if fingerprint != plan.fingerprint
            && let Some(hit) = cache.and_then(|cache| cache.get(&fingerprint))
        {
            return Ok(hit);
        }
        let (focus_ranks, focus_mode) = match focus {
            Some(focus) => (focus.ranks, Some(focus.mode)),
            None => (None, None),
        };
        let input = RenderInput {
            format: plan.format,
            version: version::label(&fingerprint),
            channels: plan
                .channels
                .into_iter()
                .filter(|channel| versions.contains_key(channel.id.as_str()))
                .collect(),
            memories: read.memories,
            categories: snapshot.categories(),
            full_budget: snapshot.budget(settings::BUDGET_TOKENS),
            titles_budget: snapshot.budget(settings::TITLES_BUDGET_TOKENS),
            overflow_line: snapshot.overflow_line(),
            focus: focus_ranks,
            focus_mode,
        };
        let estimator = Tiktoken::from_setting(&snapshot.token_estimator());
        let rendered = tokio::task::spawn_blocking(move || render(&input, &estimator))
            .await
            .map_err(|_| ApiError::internal())?;
        let rendered = Arc::new(rendered);
        if let Some(cache) = cache {
            cache.insert(fingerprint, rendered.clone());
        }
        Ok(rendered)
    }

    pub async fn get_context(
        &self,
        principal: &Principal,
        query: GetContextQuery,
        if_none_match: Option<&str>,
        header: Option<&str>,
    ) -> Result<ContextOutcome, ApiError> {
        self.authorize(principal, "get_context")?;
        let selection = Selection::parse(header, query.channels.as_deref())?;
        let focus = focus::normalize(query.focus.as_deref())?;
        let format = self.context_format(query.format);
        let snapshot = self.settings_snapshot();
        let mut settings = render_settings(&snapshot);
        // Com foco (decisão 0029), o hash leva o digest do foco, o modelo, o limiar e o modo
        // esperado; sem foco, a `version` é a de sempre.
        let expected = match &focus {
            Some(text) => {
                let (mode, model_id) = self.focus_expectation().await;
                with_focus(&mut settings, text, &model_id, &snapshot, mode);
                Some((mode, model_id))
            }
            None => None,
        };
        let mut plan = self
            .context_plan(principal, &selection, format, &settings)
            .await?;
        if let Some(tag) = if_none_match
            && version::matches(tag, &plan.version)
        {
            // O consumo do `304` sai do bloco em cache (decisão 0034); fora do cache, nada.
            let cache = match &focus {
                Some(_) => self.focus_cache(),
                None => self.context_cache(),
            };
            match cache.get(&plan.fingerprint) {
                Some(rendered) => {
                    self.consumed(
                        principal,
                        Consumption {
                            operation: "get_context",
                            context_version: Some(&plan.version),
                        },
                        &rendered.consumed,
                    )
                    .await;
                }
                None => tracing::debug!(
                    event = "context.consumed_skipped",
                    "304 sem o bloco em cache: consumo nao emitido"
                ),
            }
            return Ok(ContextOutcome::NotModified(ContextNotModified {
                not_modified: true,
                version: plan.version,
            }));
        }
        let focus_render = match (&focus, expected) {
            (Some(text), Some((mode, model_id))) => {
                let relevance = self
                    .focus_relevance(
                        text,
                        plan.channels
                            .iter()
                            .map(|channel| channel.id.clone())
                            .collect(),
                        &snapshot,
                        mode,
                    )
                    .await;
                tracing::debug!(
                    event = "context.focus",
                    focus_present = true,
                    focus_len = text.chars().count(),
                    focus_mode = relevance.mode.as_str(),
                    "foco aplicado ao contexto"
                );
                if relevance.mode != mode {
                    // Degradado: outra `version`, para nunca casar com o bloco completo.
                    with_focus(&mut settings, text, &model_id, &snapshot, relevance.mode);
                    plan.fingerprint = fingerprint_of(plan.format, &plan.reached, &settings);
                    plan.version = version::label(&plan.fingerprint);
                }
                Some(FocusRender {
                    ranks: relevance.ranks,
                    mode: relevance.mode,
                    cacheable: relevance.mode == mode,
                })
            }
            _ => None,
        };
        let rendered = self
            .context_rendered(plan, &snapshot, &settings, focus_render)
            .await?;
        self.counters()
            .record_injected(rendered.full_ids.iter().map(String::as_str));
        self.counters().record_usage(UsageEntry {
            at: ids::timestamp(ids::now()),
            app_id: principal.app.id.clone(),
            agent_id: principal.agent.as_ref().map(|agent| agent.id.clone()),
            token_id: principal.token_id.clone(),
            op: "get_context",
            channel_ids: rendered.channel_ids.clone(),
            memory_ids: rendered.full_ids.clone(),
            rendered_tokens: Some(rendered.view.rendered_tokens),
        });
        self.consumed(
            principal,
            Consumption {
                operation: "get_context",
                context_version: Some(&rendered.view.version),
            },
            &rendered.consumed,
        )
        .await;
        Ok(ContextOutcome::Rendered(rendered))
    }

    /// `version` do contexto padrão (formato `context.default_format`) da seleção e, com
    /// `with_ids`, os ids da faixa 1 (`exclude_context` da busca, 005 Q-015).
    pub(crate) async fn context_for_search(
        &self,
        principal: &Principal,
        selection: &Selection,
        with_ids: bool,
    ) -> Result<(String, Vec<String>), ApiError> {
        let format = self.context_format(None);
        let snapshot = self.settings_snapshot();
        let settings = render_settings(&snapshot);
        let plan = self
            .context_plan(principal, selection, format, &settings)
            .await?;
        if !with_ids {
            return Ok((plan.version, Vec::new()));
        }
        let version = plan.version.clone();
        let rendered = self
            .context_rendered(plan, &snapshot, &settings, None)
            .await?;
        Ok((version, rendered.full_ids.clone()))
    }

    /// Resource `engramind://channels/{slug}`: `#id: título` das memórias `active` e
    /// `permanent` do canal, na ordem do contexto, até `CHANNEL_RESOURCE_LIMIT`.
    pub async fn channel_titles(
        &self,
        principal: &Principal,
        channel: &str,
        header: Option<&str>,
    ) -> Result<String, ApiError> {
        self.authorize(principal, "list_memories")?;
        let selection = Selection::parse(header, None)?;
        let reach = self
            .require_channel(principal, &selection, channel, Scope::Read)
            .await?;
        let read = self
            .store()
            .context_memories(vec![reach.channel.id.clone()])
            .await?;
        let snapshot = self.settings_snapshot();
        let categories = snapshot.categories();
        let network = self.network().await?;
        let priority =
            snapshot.category_priorities(network.channel_settings.get(&reach.channel.id));
        let keys: Vec<String> = categories.iter().map(|c| c.key.clone()).collect();
        let mut entries: Vec<Entry<'_>> = read
            .memories
            .iter()
            .map(|memory| Entry {
                memory,
                slug: &reach.channel.slug,
                channel_priority: reach.priority,
                category_priority: priority
                    .get(memory.category.as_str())
                    .copied()
                    .unwrap_or(i64::MIN),
                focus_rank: None,
            })
            .collect();
        order::sort(&mut entries);
        let mut lines: Vec<String> = entries
            .iter()
            .take(CHANNEL_RESOURCE_LIMIT)
            .map(|entry| {
                format!(
                    "#{}: {}",
                    entry.memory.id,
                    render::escape_tagged(&entry.memory.title, &keys)
                )
            })
            .collect();
        if entries.len() > CHANNEL_RESOURCE_LIMIT {
            lines.push(format!(
                "+{} memórias: use list_memories para o restante",
                entries.len() - CHANNEL_RESOURCE_LIMIT
            ));
        }
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests;
