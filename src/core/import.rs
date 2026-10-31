//! `import_memories` (008 FR-009): traz memórias de outro sistema para um canal, preservando
//! data, autoria e origem, numa única transação e com `Idempotency-Key` obrigatória.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use super::{
    Core,
    events::EventType,
    idempotency::Mutation,
    ids,
    memories::{category, content_jobs, initial_status, memory_error, memory_text, origin},
    memory_text::{content_hash, provisional_title},
    model::{ImportResult, MemoryStatus, NewImportedMemory, Origin},
    principal::{Principal, Scope},
    selection::Selection,
};
use crate::{error::ApiError, store::Store};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportAuthor {
    /// App autor (id ou slug); precisa existir, mesmo desativado.
    #[schema(example = "legado")]
    pub app: String,
    /// Agente do app (id, `app/agente` ou só o slug do agente).
    #[schema(example = "curador")]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportItem {
    /// Um fato durável e curto, entre `memory.text_min` e `memory.text_max` caracteres.
    #[schema(example = "Rode só os testes afetados com cargo nextest.")]
    pub text: String,
    /// Uma chave de `categories.list`; omitida, vale a primeira.
    #[schema(example = "CONSTRAINTS")]
    pub category: Option<String>,
    /// `active` (padrão) ou `permanent`.
    pub status: Option<MemoryStatus>,
    /// Instante original (RFC 3339, não futuro); omitido, agora.
    #[schema(example = "2026-01-10T12:00:00Z")]
    pub created_at: Option<String>,
    /// Autor original; omitido, o token que importa.
    pub author: Option<ImportAuthor>,
    pub origin: Option<Origin>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportMemoriesBody {
    /// Canal de destino (id `ch_…` ou slug), ativo.
    #[schema(example = "eng/global")]
    pub channel: String,
    /// De 1 a `limits.max_batch` memórias.
    pub memories: Vec<ImportItem>,
}

pub(crate) fn idempotency_key_required() -> ApiError {
    ApiError::new(
        "IDEMPOTENCY_KEY_REQUIRED",
        "Esta operacao exige Idempotency-Key (idempotency_key no MCP)",
    )
}

pub(crate) fn batch_too_large(max: u32, received: usize) -> ApiError {
    ApiError::new(
        "BATCH_TOO_LARGE",
        format!("O lote aceita ate {max} itens (limits.max_batch)"),
    )
    .with_details(json!({ "max": max, "received": received }))
}

/// Acrescenta `details.index` ao erro de um item.
fn at_index(mut error: ApiError, index: usize) -> ApiError {
    let mut details = match error.details.take() {
        Some(Value::Object(details)) => details,
        Some(other) => {
            let mut details = serde_json::Map::new();
            details.insert("detail".to_owned(), other);
            details
        }
        None => serde_json::Map::new(),
    };
    details.insert("index".to_owned(), json!(index));
    error.details = Some(Value::Object(details));
    error
}

fn created_at(value: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> Result<String, ApiError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(ids::timestamp(now));
    };
    let invalid = || {
        ApiError::validation("created_at deve ser um instante RFC 3339 que nao esteja no futuro")
            .with_details(json!({ "field": "created_at" }))
    };
    let parsed = ids::parse_timestamp(value).ok_or_else(invalid)?;
    if parsed > now {
        return Err(invalid());
    }
    Ok(ids::timestamp(parsed))
}

impl<S: Store> Core<S> {
    pub async fn import_memories(
        &self,
        principal: &Principal,
        body: ImportMemoriesBody,
        key: Option<&str>,
    ) -> Result<Mutation<ImportResult>, ApiError> {
        let operation = self.authorize(principal, "import_memories")?;
        let Some(key) = key else {
            return Err(idempotency_key_required());
        };
        let snapshot = self.settings_snapshot();
        let max = snapshot.max_batch();
        if body.memories.is_empty() {
            return Err(ApiError::validation("Informe ao menos uma memoria")
                .with_details(json!({ "field": "memories" })));
        }
        if body.memories.len() > max as usize {
            return Err(batch_too_large(max, body.memories.len()));
        }
        let core = self.clone();
        let principal_owned = principal.clone();
        let input = body.clone();
        let result = self
            .mutate(
                principal,
                operation,
                Some(key),
                &input,
                201,
                |mut context| async move {
                    let selection = Selection::parse(None, None)?;
                    let reach = core
                        .require_channel(&principal_owned, &selection, &body.channel, Scope::Write)
                        .await?;
                    let channel = reach.channel;
                    let (min, max) = core.text_limits(&channel.id).await?;
                    let now = ids::now();
                    let embed = core.embeddings_enabled();
                    let mut items = Vec::with_capacity(body.memories.len());
                    // Tudo é validado antes de gravar: um item inválido recusa o lote.
                    for (index, item) in body.memories.iter().enumerate() {
                        let text = memory_text(&item.text, min, max)
                            .map_err(|error| at_index(error, index))?;
                        let (author_app_id, author_agent_id) = match &item.author {
                            Some(author) => core
                                .import_author(author)
                                .await
                                .map_err(|error| at_index(error, index))?,
                            None => (
                                principal_owned.app.id.clone(),
                                principal_owned.agent.as_ref().map(|agent| agent.id.clone()),
                            ),
                        };
                        items.push(NewImportedMemory {
                            index,
                            category: category(&snapshot, item.category.as_deref())
                                .map_err(|error| at_index(error, index))?,
                            title: provisional_title(&text),
                            content_hash: content_hash(&text),
                            status: initial_status(item.status)
                                .map_err(|error| at_index(error, index))?,
                            origin: origin(item.origin.as_ref())
                                .map_err(|error| at_index(error, index))?,
                            created_at: created_at(item.created_at.as_deref(), now)
                                .map_err(|error| at_index(error, index))?,
                            author_app_id,
                            author_agent_id,
                            jobs: content_jobs(false, embed),
                            text,
                        });
                    }
                    context.events = core
                        .channel_event(EventType::MemoryCreated, &channel.slug)
                        .await?;
                    core.store()
                        .import_memories(channel.id, items, snapshot.dedupe_exact(), context)
                        .await
                        .map_err(memory_error)
                },
            )
            .await;
        if let Ok(mutation) = &result
            && !mutation.replayed
        {
            self.network_changed();
            tracing::info!(
                event = "import.completed",
                created = mutation.value.created.len(),
                duplicates = mutation.value.duplicates.len(),
                "importacao concluida"
            );
        }
        result
    }

    /// App e agente autores de um item importado (Q-003).
    async fn import_author(
        &self,
        author: &ImportAuthor,
    ) -> Result<(String, Option<String>), ApiError> {
        let app = self.find_app(&author.app).await?;
        let Some(agent) = author
            .agent
            .as_deref()
            .map(str::trim)
            .filter(|agent| !agent.is_empty())
        else {
            return Ok((app.id, None));
        };
        let lookup = if agent.contains('/') || agent.starts_with("ag_") {
            agent.to_owned()
        } else {
            format!("{}/{agent}", app.slug)
        };
        let agent = self.find_agent(&lookup).await?;
        if agent.app.id != app.id {
            return Err(super::identities::agent_not_found());
        }
        Ok((app.id, Some(agent.id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_at_rejects_the_future_and_normalizes() {
        let now = ids::parse_timestamp("2026-06-01T00:00:00Z").expect("now");
        assert_eq!(
            created_at(Some("2026-01-10T12:00:00Z"), now).expect("ok"),
            "2026-01-10T12:00:00.000Z"
        );
        assert_eq!(created_at(None, now).expect("now"), ids::timestamp(now));
        assert_eq!(
            created_at(Some("2027-01-01T00:00:00Z"), now)
                .expect_err("futuro")
                .code,
            "VALIDATION_ERROR"
        );
    }

    #[test]
    fn item_errors_carry_the_index() {
        let error = at_index(
            ApiError::validation("x").with_details(json!({ "field": "text" })),
            3,
        );
        assert_eq!(error.details, Some(json!({ "field": "text", "index": 3 })));
    }
}
