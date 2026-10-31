//! Memórias (desenho §6 e §7): criação, edição versionada, status, `move`, `revert`, `delete`,
//! leitura e histórico. Todo acesso passa por `core::access`; uma memória num canal que o
//! token não alcança responde `404 MEMORY_NOT_FOUND` (Q-005).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::{IntoParams, ToSchema};

use super::{
    Core,
    access::Purpose,
    events::EventType,
    idempotency::Mutation,
    identities::{agent_lookup, app_lookup, next_cursor, optional_text, text},
    ids,
    memory_text::{content_hash, jaccard, provisional_title, query_terms},
    model::{
        AgentLookup, Lookup, Memory, MemoryContent, MemoryFilter, MemoryPatch, MemoryRevision,
        MemoryStatus, MemoryTarget, MemoryTitle, MemoryWrite, NewJob, NewMemory, Origin,
        SimilarMemory, StatusAction, TextQuery,
    },
    pagination::PageRequest,
    principal::{Principal, Scope},
    selection::Selection,
    settings::SettingsSnapshot,
    store_error,
};
use crate::{
    error::ApiError,
    store::{Store, StoreError},
};

const MAX_ORIGIN_SOURCE: usize = 64;
const MAX_ORIGIN_LABEL: usize = 200;
/// Candidatos do FTS avaliados para `similar` (Q-003).
const SIMILAR_CANDIDATES: u32 = 10;
const SIMILAR_MAX: usize = 3;

// ---------- entradas ----------

/// `full` (padrão) devolve a memória inteira; `title`, só `id`, `title`, `channel`,
/// `category` e `status`.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Fields {
    #[default]
    Full,
    Title,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct ListMemoriesQuery {
    /// Slugs e prefixos (`eng/global,eng/projeto-*`) separados por vírgula; restringe dentro do
    /// que o token alcança e do cabeçalho `Engramind-Channels`.
    pub channels: Option<String>,
    /// Categorias separadas por vírgula.
    pub categories: Option<String>,
    /// `active`, `permanent` e/ou `archived`, separados por vírgula (padrão
    /// `active,permanent`).
    pub status: Option<String>,
    /// Autor: app (`app_…` ou slug) ou agente (`ag_…` ou `app/agente`).
    pub author: Option<String>,
    /// Só as alteradas a partir deste instante (RFC 3339).
    pub since: Option<String>,
    pub fields: Option<Fields>,
    /// Cursor opaco devolvido em `next_cursor`.
    pub cursor: Option<String>,
    /// Itens por página, de 1 a 200 (padrão 50).
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct HistoryQuery {
    /// Cursor opaco devolvido em `next_cursor`.
    pub cursor: Option<String>,
    /// Revisões por página, de 1 a 200 (padrão 50).
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMemoryBody {
    /// Canal (id `ch_…` ou slug) com `write` efetivo.
    #[schema(example = "eng/global")]
    pub channel: String,
    /// Um fato durável e curto, entre `memory.text_min` e `memory.text_max` caracteres.
    #[schema(
        example = "Rode só os testes afetados com cargo nextest; a suíte completa fica no CI."
    )]
    pub text: String,
    /// Uma chave de `categories.list`; omitida, vale a primeira.
    #[schema(example = "CONSTRAINTS")]
    pub category: Option<String>,
    pub origin: Option<Origin>,
    /// `active` (padrão) ou `permanent`.
    pub status: Option<MemoryStatus>,
}

/// Campos omitidos não mudam; informe ao menos `text` ou `category`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMemoryBody {
    pub text: Option<String>,
    #[schema(example = "ARCHITECTURE")]
    pub category: Option<String>,
    /// Versão lida; divergente → `409 MEMORY_VERSION_CONFLICT` com `details.current_version`.
    #[schema(example = 1)]
    pub expected_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveMemoryBody {
    /// Canal de destino (id `ch_…` ou slug) com `write` efetivo.
    #[schema(example = "eng/projeto-atlas")]
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevertMemoryBody {
    /// Versão do histórico cujo texto, título e categoria voltam numa revisão nova.
    #[schema(example = 1, minimum = 1)]
    pub version: i64,
}

// ---------- saídas ----------

/// Memória inteira (`fields=full`) ou só o título (`fields=title`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum MemoryItem {
    Full(Box<Memory>),
    Title(MemoryTitle),
}

impl MemoryItem {
    pub fn of(memory: Memory, fields: Fields) -> Self {
        match fields {
            Fields::Full => Self::Full(Box::new(memory)),
            Fields::Title => Self::Title(memory.title_view()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MemoryList {
    /// Da alteração mais recente à mais antiga.
    pub memories: Vec<MemoryItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MemoryHistory {
    #[schema(example = "m_8f2a1c3d")]
    pub memory_id: String,
    /// Da versão mais nova à mais antiga.
    pub revisions: Vec<MemoryRevision>,
    pub next_cursor: Option<String>,
}

// ---------- validação ----------

pub fn memory_not_found() -> ApiError {
    ApiError::not_found("MEMORY_NOT_FOUND", "Memoria nao encontrada")
}

fn revision_not_found() -> ApiError {
    ApiError::not_found("REVISION_NOT_FOUND", "Revisao nao encontrada")
}

/// Id `m_` + 8 hex; outro formato → `400 VALIDATION_ERROR`.
pub fn memory_id(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if ids::is_valid(ids::MEMORY_PREFIX, value) {
        Ok(value.to_owned())
    } else {
        Err(ApiError::validation("memoria deve ser um id m_..."))
    }
}

/// Texto sem espaços nas pontas, entre `min` e `max` caracteres (FR-002).
pub fn memory_text(value: &str, min: usize, max: usize) -> Result<String, ApiError> {
    let value = value.trim();
    let length = value.chars().count();
    if (min..=max).contains(&length) {
        return Ok(value.to_owned());
    }
    Err(ApiError::validation(format!(
        "text deve ter de {min} a {max} caracteres (tem {length})"
    ))
    .with_details(json!({ "field": "text", "min": min, "max": max, "length": length })))
}

/// Categoria de `categories.list`; omitida, a primeira (FR-003).
pub fn category(snapshot: &SettingsSnapshot, value: Option<&str>) -> Result<String, ApiError> {
    let categories = snapshot.categories();
    let Some(value) = value.map(str::trim) else {
        return categories
            .first()
            .map(|category| category.key.clone())
            .ok_or_else(ApiError::internal);
    };
    if categories.iter().any(|category| category.key == value) {
        return Ok(value.to_owned());
    }
    Err(ApiError::new(
        "CATEGORY_NOT_FOUND",
        format!("Categoria inexistente: {value}"),
    )
    .with_details(json!({
        "category": value,
        "valid": categories.iter().map(|category| category.key.as_str()).collect::<Vec<_>>(),
    })))
}

pub(crate) fn origin(value: Option<&Origin>) -> Result<Option<Origin>, ApiError> {
    value
        .map(|origin| {
            Ok(Origin {
                source: text("origin.source", &origin.source, MAX_ORIGIN_SOURCE)?,
                label: optional_text("origin.label", origin.label.as_deref(), MAX_ORIGIN_LABEL)?,
            })
        })
        .transpose()
}

pub(crate) fn initial_status(value: Option<MemoryStatus>) -> Result<MemoryStatus, ApiError> {
    match value.unwrap_or(MemoryStatus::Active) {
        MemoryStatus::Archived => Err(ApiError::validation(
            "status inicial deve ser active ou permanent",
        )),
        status => Ok(status),
    }
}

fn statuses(value: Option<&str>) -> Result<Vec<MemoryStatus>, ApiError> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(vec![MemoryStatus::Active, MemoryStatus::Permanent]);
    };
    let mut parsed = Vec::new();
    for item in value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let status = MemoryStatus::parse(item)
            .ok_or_else(|| ApiError::validation("status aceita active, permanent e archived"))?;
        if !parsed.contains(&status) {
            parsed.push(status);
        }
    }
    Ok(parsed)
}

fn csv(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Jobs de conteúdo (Q-001): título e texto pelo Redator, e o vetor (só com os embeddings
/// ligados, 007 FR-010). `title_only` no `revert` (Q-008), para o Redator não reescrever o
/// texto restaurado.
pub(crate) fn content_jobs(title_only: bool, embed: bool) -> Vec<NewJob> {
    let mut extra = Map::new();
    if title_only {
        extra.insert("title_only".to_owned(), Value::Bool(true));
    }
    let mut jobs = vec![NewJob {
        kind: "redact",
        extra,
    }];
    if embed {
        jobs.push(NewJob {
            kind: "embed",
            extra: Map::new(),
        });
    }
    jobs
}

/// Erros do `Store` numa mutação de memória.
pub(crate) fn memory_error(error: StoreError) -> ApiError {
    store_error(error, memory_not_found)
}

// ---------- operações ----------

impl<S: Store> Core<S> {
    /// A memória e o alcance do token no canal dela, com o escopo exigido. Fora do alcance ou
    /// da seleção → `404 MEMORY_NOT_FOUND`; sem o escopo → `403 SCOPE_REQUIRED`.
    async fn reach_memory(
        &self,
        principal: &Principal,
        header: Option<&str>,
        id: &str,
        scope: Scope,
    ) -> Result<Memory, ApiError> {
        let selection = Selection::parse(header, None)?;
        let memory = self
            .store()
            .get_memory(memory_id(id)?)
            .await?
            .ok_or_else(memory_not_found)?;
        match self
            .require_channel(principal, &selection, &memory.channel_id, scope)
            .await
        {
            Ok(_) => Ok(memory),
            Err(error) if matches!(error.code, "CHANNEL_NOT_SUBSCRIBED" | "CHANNEL_NOT_FOUND") => {
                Err(memory_not_found())
            }
            Err(error) => Err(error),
        }
    }

    /// `memory.text_max` do canal (com a sobreposição dele).
    pub(crate) async fn text_limits(&self, channel_id: &str) -> Result<(usize, usize), ApiError> {
        let snapshot = self.settings_snapshot();
        let network = self.network().await?;
        Ok((
            snapshot.text_min(),
            snapshot.text_max(network.channel_settings.get(channel_id)),
        ))
    }

    /// Até três memórias ativas do canal parecidas com o texto (Q-003): candidatos pelo FTS e
    /// nota de Jaccard dos termos, a partir de `memory.similar_threshold`. A própria memória e o
    /// texto idêntico ficam de fora.
    pub(crate) async fn similar(
        &self,
        channel_id: &str,
        text: &str,
        hash: &[u8],
        exclude: Option<&str>,
    ) -> Result<Vec<SimilarMemory>, ApiError> {
        let candidates = self
            .store()
            .search_text(TextQuery {
                terms: query_terms(text),
                channel_ids: vec![channel_id.to_owned()],
                categories: Vec::new(),
                statuses: vec![MemoryStatus::Active, MemoryStatus::Permanent],
                exclude_ids: exclude.map(str::to_owned).into_iter().collect(),
                limit: SIMILAR_CANDIDATES,
            })
            .await?;
        let threshold = self.settings_snapshot().similar_threshold();
        let mut similar: Vec<SimilarMemory> = candidates
            .into_iter()
            .filter(|memory| content_hash(&memory.text) != hash)
            .map(|memory| (jaccard(text, &memory.text), memory))
            .filter(|(score, _)| *score >= threshold)
            .map(|(score, memory)| SimilarMemory {
                id: memory.id,
                title: memory.title,
                channel: memory.channel,
                score: (score * 1000.0).round() / 1000.0,
            })
            .collect();
        similar.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        similar.truncate(SIMILAR_MAX);
        Ok(similar)
    }

    /// `409 MEMORY_DUPLICATE` com o id da memória repetida.
    async fn duplicate(&self, channel_id: &str, text: &str, exclude: &str) -> ApiError {
        let existing = self
            .store()
            .find_memory_by_hash(
                channel_id.to_owned(),
                content_hash(text),
                Some(exclude.to_owned()),
            )
            .await
            .ok()
            .flatten();
        ApiError::new(
            "MEMORY_DUPLICATE",
            "Ja existe uma memoria com esse texto no canal",
        )
        .with_details(json!({ "existing_id": existing }))
    }

    /// `409 MEMORY_VERSION_CONFLICT` com a versão atual.
    async fn version_conflict(&self, id: &str) -> ApiError {
        let current = self
            .store()
            .get_memory(id.to_owned())
            .await
            .ok()
            .flatten()
            .map(|memory| memory.version);
        ApiError::new(
            "MEMORY_VERSION_CONFLICT",
            "A memoria mudou desde a versao informada; releia e tente de novo",
        )
        .with_details(json!({ "current_version": current }))
    }

    pub async fn list_memories(
        &self,
        principal: &Principal,
        query: ListMemoriesQuery,
        header: Option<&str>,
    ) -> Result<MemoryList, ApiError> {
        self.authorize(principal, "list_memories")?;
        let selection = Selection::parse(header, query.channels.as_deref())?;
        let page = PageRequest::new(query.cursor.as_deref(), query.limit)?;
        let statuses = statuses(query.status.as_deref())?;
        let since = query
            .since
            .as_deref()
            .map(|value| {
                ids::parse_timestamp(value.trim())
                    .map(ids::timestamp)
                    .ok_or_else(|| ApiError::validation("since deve ser um instante RFC 3339"))
            })
            .transpose()?;
        let empty = MemoryList {
            memories: Vec::new(),
            next_cursor: None,
        };
        let (author_app_id, author_agent_id) = match query.author.as_deref().map(str::trim) {
            None | Some("") => (None, None),
            Some(author) if author.starts_with(ids::AGENT_PREFIX) || author.contains('/') => {
                let found = match agent_lookup(author)? {
                    AgentLookup::Id(id) => Some(id),
                    lookup => self.store().get_agent(lookup).await?.map(|agent| agent.id),
                };
                match found {
                    Some(id) => (None, Some(id)),
                    None => return Ok(empty),
                }
            }
            Some(author) => match app_lookup(author) {
                Lookup::Id(id) => (Some(id), None),
                lookup => match self.store().get_app(lookup).await? {
                    Some(app) => (Some(app.id), None),
                    None => return Ok(empty),
                },
            },
        };
        let reaches = self
            .resolve(principal, &selection, Purpose::Explicit)
            .await?;
        let filter = MemoryFilter {
            channel_ids: reaches
                .iter()
                .map(|reach| reach.channel.id.clone())
                .collect(),
            categories: csv(query.categories.as_deref()),
            statuses,
            author_app_id,
            author_agent_id,
            since,
        };
        let page = self.store().list_memories(filter, page).await?;
        let fields = query.fields.unwrap_or_default();
        Ok(MemoryList {
            next_cursor: next_cursor(&page),
            memories: page
                .items
                .into_iter()
                .map(|memory| MemoryItem::of(memory, fields))
                .collect(),
        })
    }

    pub async fn get_memory(
        &self,
        principal: &Principal,
        memory: &str,
        header: Option<&str>,
    ) -> Result<Memory, ApiError> {
        self.authorize(principal, "get_memory")?;
        self.reach_memory(principal, header, memory, Scope::Read)
            .await
    }

    pub async fn memory_history(
        &self,
        principal: &Principal,
        memory: &str,
        query: HistoryQuery,
        header: Option<&str>,
    ) -> Result<MemoryHistory, ApiError> {
        self.authorize(principal, "memory_history")?;
        let page = PageRequest::new(query.cursor.as_deref(), query.limit)?;
        let memory = self
            .reach_memory(principal, header, memory, Scope::Read)
            .await?;
        let page = self.store().memory_history(memory.id.clone(), page).await?;
        Ok(MemoryHistory {
            memory_id: memory.id,
            next_cursor: next_cursor(&page),
            revisions: page.items,
        })
    }

    pub async fn create_memory(
        &self,
        principal: &Principal,
        body: CreateMemoryBody,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<MemoryWrite>, ApiError> {
        let operation = self.authorize(principal, "create_memory")?;
        let snapshot = self.settings_snapshot();
        let status = initial_status(body.status)?;
        let origin = origin(body.origin.as_ref())?;
        let category = category(&snapshot, body.category.as_deref())?;
        let selection = Selection::parse(header, None)?;
        let core = self.clone();
        let principal_owned = principal.clone();
        let input = body.clone();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                201,
                |mut context| async move {
                    let reach = core
                        .require_channel(&principal_owned, &selection, &body.channel, Scope::Write)
                        .await?;
                    context.events = core
                        .channel_event(EventType::MemoryCreated, &reach.channel.slug)
                        .await?;
                    let channel_id = reach.channel.id;
                    let (min, max) = core.text_limits(&channel_id).await?;
                    let text = memory_text(&body.text, min, max)?;
                    let hash = content_hash(&text);
                    let similar = core.similar(&channel_id, &text, &hash, None).await?;
                    let new = NewMemory {
                        channel_id,
                        category,
                        title: provisional_title(&text),
                        text,
                        status,
                        content_hash: hash,
                        origin,
                        dedupe: snapshot.dedupe_exact(),
                        jobs: content_jobs(false, core.embeddings_enabled()),
                        similar,
                    };
                    core.store()
                        .create_memory(new, context)
                        .await
                        .map_err(memory_error)
                },
            )
            .await;
        self.network_changed();
        self.jobs_enqueued();
        result
    }

    pub async fn update_memory(
        &self,
        principal: &Principal,
        memory: &str,
        body: UpdateMemoryBody,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<MemoryWrite>, ApiError> {
        let operation = self.authorize(principal, "update_memory")?;
        if body.text.is_none() && body.category.is_none() {
            return Err(ApiError::validation("Informe text ou category"));
        }
        let snapshot = self.settings_snapshot();
        let category = body
            .category
            .as_deref()
            .map(|value| category(&snapshot, Some(value)))
            .transpose()?;
        let id = memory_id(memory)?;
        let input = json!({ "memory": id, "body": body });
        let core = self.clone();
        let principal_owned = principal.clone();
        let header = header.map(str::to_owned);
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |mut context| async move {
                    let memory = core
                        .reach_memory(&principal_owned, header.as_deref(), &id, Scope::Write)
                        .await?;
                    context.events = core
                        .channel_event(EventType::MemoryUpdated, &memory.channel)
                        .await?;
                    let content = match &body.text {
                        Some(value) => {
                            let (min, max) = core.text_limits(&memory.channel_id).await?;
                            let text = memory_text(value, min, max)?;
                            Some(MemoryContent {
                                title: provisional_title(&text),
                                content_hash: content_hash(&text),
                                text,
                            })
                        }
                        None => None,
                    };
                    let (text, hash) = match &content {
                        Some(content) => (content.text.clone(), content.content_hash.clone()),
                        None => (memory.text.clone(), content_hash(&memory.text)),
                    };
                    let similar = core
                        .similar(&memory.channel_id, &text, &hash, Some(&memory.id))
                        .await?;
                    let patch = MemoryPatch {
                        expected_version: body.expected_version,
                        content,
                        category,
                        dedupe: snapshot.dedupe_exact(),
                        reason: "update",
                        jobs: content_jobs(false, core.embeddings_enabled()),
                        similar,
                    };
                    let target = MemoryTarget {
                        id: memory.id.clone(),
                        channel_id: memory.channel_id.clone(),
                    };
                    match core.store().update_memory(target, patch, context).await {
                        Err(StoreError::Conflict("MEMORY_VERSION_CONFLICT")) => {
                            Err(core.version_conflict(&memory.id).await)
                        }
                        Err(StoreError::Conflict("MEMORY_DUPLICATE")) => {
                            Err(core.duplicate(&memory.channel_id, &text, &memory.id).await)
                        }
                        other => other.map_err(memory_error),
                    }
                },
            )
            .await;
        self.network_changed();
        self.jobs_enqueued();
        result
    }

    pub async fn revert_memory(
        &self,
        principal: &Principal,
        memory: &str,
        body: RevertMemoryBody,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<Memory>, ApiError> {
        let operation = self.authorize(principal, "revert_memory")?;
        if body.version < 1 {
            return Err(ApiError::validation("version deve ser 1 ou maior"));
        }
        let id = memory_id(memory)?;
        let input = json!({ "memory": id, "body": body });
        let dedupe = self.settings_snapshot().dedupe_exact();
        let core = self.clone();
        let principal_owned = principal.clone();
        let header = header.map(str::to_owned);
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |mut context| async move {
                    let memory = core
                        .reach_memory(&principal_owned, header.as_deref(), &id, Scope::Write)
                        .await?;
                    context.events = core
                        .channel_event(EventType::MemoryUpdated, &memory.channel)
                        .await?;
                    let revision = core
                        .store()
                        .get_revision(memory.id.clone(), body.version)
                        .await?
                        .ok_or_else(revision_not_found)?;
                    let text = revision.text.clone();
                    let patch = MemoryPatch {
                        expected_version: Some(memory.version),
                        content: Some(MemoryContent {
                            title: revision.title,
                            content_hash: content_hash(&revision.text),
                            text: revision.text,
                        }),
                        category: Some(revision.category),
                        dedupe,
                        reason: "revert",
                        jobs: content_jobs(true, core.embeddings_enabled()),
                        similar: Vec::new(),
                    };
                    let target = MemoryTarget {
                        id: memory.id.clone(),
                        channel_id: memory.channel_id.clone(),
                    };
                    match core.store().update_memory(target, patch, context).await {
                        Err(StoreError::Conflict("MEMORY_VERSION_CONFLICT")) => {
                            Err(core.version_conflict(&memory.id).await)
                        }
                        Err(StoreError::Conflict("MEMORY_DUPLICATE")) => {
                            Err(core.duplicate(&memory.channel_id, &text, &memory.id).await)
                        }
                        other => other.map(|write| write.memory).map_err(memory_error),
                    }
                },
            )
            .await;
        self.network_changed();
        self.jobs_enqueued();
        result
    }

    pub async fn archive_memory(
        &self,
        principal: &Principal,
        memory: &str,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<Memory>, ApiError> {
        self.set_status(
            principal,
            "archive_memory",
            StatusAction::Archive,
            memory,
            header,
            key,
        )
        .await
    }

    pub async fn restore_memory(
        &self,
        principal: &Principal,
        memory: &str,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<Memory>, ApiError> {
        self.set_status(
            principal,
            "restore_memory",
            StatusAction::Restore,
            memory,
            header,
            key,
        )
        .await
    }

    pub async fn pin_memory(
        &self,
        principal: &Principal,
        memory: &str,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<Memory>, ApiError> {
        self.set_status(
            principal,
            "pin_memory",
            StatusAction::Pin,
            memory,
            header,
            key,
        )
        .await
    }

    pub async fn unpin_memory(
        &self,
        principal: &Principal,
        memory: &str,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<Memory>, ApiError> {
        self.set_status(
            principal,
            "unpin_memory",
            StatusAction::Unpin,
            memory,
            header,
            key,
        )
        .await
    }

    async fn set_status(
        &self,
        principal: &Principal,
        name: &str,
        action: StatusAction,
        memory: &str,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<Memory>, ApiError> {
        let operation = self.authorize(principal, name)?;
        let id = memory_id(memory)?;
        let input = json!({ "memory": id });
        let core = self.clone();
        let principal_owned = principal.clone();
        let header = header.map(str::to_owned);
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |mut context| async move {
                    let memory = core
                        .reach_memory(&principal_owned, header.as_deref(), &id, Scope::Write)
                        .await?;
                    let kind = match action {
                        StatusAction::Archive => EventType::MemoryArchived,
                        _ => EventType::MemoryUpdated,
                    };
                    context.events = core.channel_event(kind, &memory.channel).await?;
                    let target = MemoryTarget {
                        id: memory.id,
                        channel_id: memory.channel_id,
                    };
                    core.store()
                        .set_memory_status(target, action, context)
                        .await
                        .map_err(memory_error)
                },
            )
            .await;
        self.network_changed();
        self.jobs_enqueued();
        result
    }

    pub async fn move_memory(
        &self,
        principal: &Principal,
        memory: &str,
        body: MoveMemoryBody,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<Memory>, ApiError> {
        let operation = self.authorize(principal, "move_memory")?;
        let id = memory_id(memory)?;
        let input = json!({ "memory": id, "body": body });
        let dedupe = self.settings_snapshot().dedupe_exact();
        let selection = Selection::parse(header, None)?;
        let core = self.clone();
        let principal_owned = principal.clone();
        let header = header.map(str::to_owned);
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |mut context| async move {
                    let memory = core
                        .reach_memory(&principal_owned, header.as_deref(), &id, Scope::Write)
                        .await?;
                    let destination = core
                        .require_channel(&principal_owned, &selection, &body.to, Scope::Write)
                        .await?
                        .channel;
                    // Um evento por canal envolvido (FR-007).
                    let mut events = core
                        .channel_event(EventType::MemoryUpdated, &memory.channel)
                        .await?;
                    if destination.slug != memory.channel {
                        events.extend(
                            core.channel_event(EventType::MemoryUpdated, &destination.slug)
                                .await?,
                        );
                    }
                    context.events = events;
                    let destination = destination.id;
                    let target = MemoryTarget {
                        id: memory.id.clone(),
                        channel_id: memory.channel_id.clone(),
                    };
                    match core
                        .store()
                        .move_memory(target, destination.clone(), dedupe, context)
                        .await
                    {
                        Err(StoreError::Conflict("MEMORY_DUPLICATE")) => {
                            Err(core.duplicate(&destination, &memory.text, &memory.id).await)
                        }
                        other => other.map_err(memory_error),
                    }
                },
            )
            .await;
        self.network_changed();
        self.jobs_enqueued();
        result
    }

    pub async fn delete_memory(
        &self,
        principal: &Principal,
        memory: &str,
        header: Option<&str>,
        key: Option<&str>,
    ) -> Result<Mutation<Memory>, ApiError> {
        let operation = self.authorize(principal, "delete_memory")?;
        let id = memory_id(memory)?;
        let input = json!({ "memory": id });
        let core = self.clone();
        let principal_owned = principal.clone();
        let header = header.map(str::to_owned);
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |mut context| async move {
                    let memory = core
                        .reach_memory(&principal_owned, header.as_deref(), &id, Scope::Delete)
                        .await?;
                    context.events = core
                        .channel_event(EventType::MemoryDeleted, &memory.channel)
                        .await?;
                    let target = MemoryTarget {
                        id: memory.id,
                        channel_id: memory.channel_id,
                    };
                    core.store()
                        .delete_memory(target, context)
                        .await
                        .map_err(memory_error)
                },
            )
            .await;
        self.network_changed();
        self.jobs_enqueued();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_trimmed_and_bounded() {
        assert_eq!(memory_text("  abc  ", 3, 10).expect("ok"), "abc");
        let error = memory_text("ab", 3, 10).expect_err("short");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert_eq!(error.details.expect("details")["field"], "text");
        assert!(memory_text(&"a".repeat(11), 3, 10).is_err());
        assert!(memory_text("ção", 3, 3).is_ok());
    }

    #[test]
    fn categories_default_to_the_first_and_must_exist() {
        let snapshot = SettingsSnapshot::default();
        assert_eq!(category(&snapshot, None).expect("default"), "PROJECT_RULES");
        assert_eq!(
            category(&snapshot, Some(" NAMING ")).expect("naming"),
            "NAMING"
        );
        assert_eq!(
            category(&snapshot, Some("OUTRA")).expect_err("x").code,
            "CATEGORY_NOT_FOUND"
        );
    }

    #[test]
    fn statuses_default_to_active_and_permanent() {
        assert_eq!(
            statuses(None).expect("default"),
            vec![MemoryStatus::Active, MemoryStatus::Permanent]
        );
        assert_eq!(
            statuses(Some("archived, archived")).expect("archived"),
            vec![MemoryStatus::Archived]
        );
        assert!(statuses(Some("deleted")).is_err());
        assert!(initial_status(Some(MemoryStatus::Archived)).is_err());
        assert_eq!(initial_status(None).expect("active"), MemoryStatus::Active);
    }

    #[test]
    fn ids_and_origins_are_validated() {
        assert!(memory_id("m_0a1b2c3d").is_ok());
        assert!(memory_id("m_xyz").is_err());
        let blank = Origin {
            source: " ".to_owned(),
            label: None,
        };
        assert!(origin(Some(&blank)).is_err());
        let ok = Origin {
            source: "curator".to_owned(),
            label: Some("  ".to_owned()),
        };
        assert_eq!(origin(Some(&ok)).expect("ok").expect("some").label, None);
    }

    #[test]
    fn revert_jobs_only_retitle() {
        let jobs = content_jobs(true, true);
        assert_eq!(jobs[0].kind, "redact");
        assert_eq!(jobs[0].extra["title_only"], true);
        assert_eq!(jobs[1].kind, "embed");
        assert!(content_jobs(false, true)[0].extra.is_empty());
        assert_eq!(content_jobs(false, false).len(), 1);
    }
}
