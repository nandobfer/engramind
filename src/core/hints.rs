//! `get_hints` (desenho §9): a "lembrança vaga". A mesma busca vetorial com o limiar alto
//! `search.hint_threshold` sobre o cosseno, só em canais `inject` e `search` (nunca `manage`,
//! nem citado), sem o que já está no contexto; devolve `id`, título, canal e a nota
//! (`score`, `method`, `duplicate`). Sem embeddings, lista vazia: uma dica nunca é ruído
//! textual (o `similar` textual fica com `get_memory`, decisão 0031).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    Core,
    access::Purpose,
    consumption::{Consumption, group},
    ids,
    model::{MemoryStatus, Mode, SimilarMethod, UsageEntry, VectorQuery},
    principal::Principal,
    selection::Selection,
    settings::{SEARCH_HINT_LIMIT, SEARCH_HINT_THRESHOLD, VECTOR_DUPLICATE_THRESHOLD},
};
use crate::{error::ApiError, store::Store};

pub const MAX_LIMIT: u32 = 20;
const MAX_TEXT: usize = 2000;
/// Candidatos do ramo vetorial por dica pedida, antes do limiar.
const CANDIDATES_PER_HINT: u32 = 4;
const MIN_CANDIDATES: u32 = 10;

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HintsBody {
    /// Texto do turno (1 a 2000 caracteres), por exemplo a mensagem do usuário.
    #[schema(example = "como rodo os testes?")]
    pub text: String,
    /// Slugs e prefixos (`eng/*`); restringe dentro do alcance e do `Engramind-Channels`.
    /// Canais `manage` nunca entram.
    #[schema(example = json!(["eng/global"]))]
    pub channels: Option<Vec<String>>,
    /// De 1 a 20 (padrão `search.hint_limit`).
    #[schema(minimum = 1, maximum = 20)]
    pub limit: Option<u32>,
}

/// `#id · título · canal`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Hint {
    #[schema(example = "m_8f2a1c3d")]
    pub id: String,
    #[schema(example = "Suíte completa com nextest")]
    pub title: String,
    #[schema(example = "eng/global")]
    pub channel: String,
    /// Cosseno (`vector`) ou Jaccard dos termos (`text`), de 0 a 1.
    #[schema(example = 0.87)]
    pub score: f64,
    pub method: SimilarMethod,
    /// Cosseno ≥ `memory.vector_duplicate_threshold` (só `vector`): o texto do turno repete a
    /// memória; informativo.
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HintsResult {
    /// Da mais parecida à menos parecida.
    pub hints: Vec<Hint>,
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

impl<S: Store> Core<S> {
    pub async fn get_hints(
        &self,
        principal: &Principal,
        body: HintsBody,
        header: Option<&str>,
    ) -> Result<HintsResult, ApiError> {
        self.authorize(principal, "get_hints")?;
        let text = body.text.trim();
        let length = text.chars().count();
        if length == 0 || length > MAX_TEXT {
            return Err(ApiError::validation(format!(
                "text deve ter de 1 a {MAX_TEXT} caracteres"
            )));
        }
        let snapshot = self.settings_snapshot();
        let limit = body
            .limit
            .unwrap_or_else(|| u32::try_from(snapshot.whole(SEARCH_HINT_LIMIT)).unwrap_or(3));
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(ApiError::validation(format!(
                "limit deve estar entre 1 e {MAX_LIMIT}"
            )));
        }
        let channels = body.channels.map(|channels| channels.join(","));
        let selection = Selection::parse(header, channels.as_deref())?;
        let reaches = self.resolve(principal, &selection, Purpose::Search).await?;
        let channel_ids: Vec<String> = reaches
            .iter()
            .filter(|reach| matches!(reach.mode, Mode::Inject | Mode::Search))
            .map(|reach| reach.channel.id.clone())
            .collect();
        if channel_ids.is_empty() {
            return Ok(HintsResult { hints: Vec::new() });
        }
        let (_, in_context) = self.context_for_search(principal, &selection, true).await?;
        let statuses = vec![MemoryStatus::Active, MemoryStatus::Permanent];
        let hints: Vec<Hint> = match self.query_vector(text, "get_hints").await {
            Some((runtime, embedding)) => {
                let threshold = snapshot.number(SEARCH_HINT_THRESHOLD);
                let duplicate = snapshot.number(VECTOR_DUPLICATE_THRESHOLD);
                self.store()
                    .search_vectors(VectorQuery {
                        embedding,
                        model_id: runtime.model_id,
                        channel_ids: channel_ids.clone(),
                        categories: Vec::new(),
                        tags: Vec::new(),
                        statuses,
                        exclude_ids: in_context,
                        limit: (limit * CANDIDATES_PER_HINT).max(MIN_CANDIDATES),
                    })
                    .await?
                    .into_iter()
                    .map(|hit| (1.0 - hit.distance, hit.memory))
                    .filter(|(score, _)| *score >= threshold)
                    .take(limit as usize)
                    .map(|(score, memory)| Hint {
                        id: memory.id,
                        title: memory.title,
                        channel: memory.channel,
                        score: round3(score),
                        method: SimilarMethod::Vector,
                        duplicate: score >= duplicate,
                    })
                    .collect()
            }
            // Sem embeddings, lista vazia: uma dica nunca é ruído textual.
            None => Vec::new(),
        };
        let at = ids::timestamp(ids::now());
        self.counters()
            .record(hints.iter().map(|hint| hint.id.as_str()), &at);
        self.counters().record_usage(UsageEntry {
            at,
            app_id: principal.app.id.clone(),
            agent_id: principal.agent.as_ref().map(|agent| agent.id.clone()),
            token_id: principal.token_id.clone(),
            op: "get_hints",
            channel_ids,
            memory_ids: hints.iter().map(|hint| hint.id.clone()).collect(),
            rendered_tokens: None,
        });
        self.consumed(
            principal,
            Consumption {
                operation: "get_hints",
                context_version: None,
            },
            &group(
                hints
                    .iter()
                    .map(|hint| (hint.channel.as_str(), hint.id.as_str())),
            ),
        )
        .await;
        Ok(HintsResult { hints })
    }
}
