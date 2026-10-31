//! `search_memories` (desenho §9): ramo vetorial (cosseno do sqlite-vec, só o modelo vigente) e
//! ramo textual (FTS5/BM25 sem acentos) sobre os canais em modo `inject` e `search`, mais os
//! `manage` citados, unidos pela fusão RRF ponderada. Sem embeddings, só o FTS (`mode: text`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::settings::{
    SEARCH_DEFAULT_LIMIT, SEARCH_RRF_K, SEARCH_TEXT_WEIGHT, SEARCH_VECTOR_WEIGHT,
};
use super::{
    Core,
    access::Purpose,
    ids,
    memories::{Fields, MemoryItem},
    memory_text::query_terms,
    model::{Memory, MemoryStatus, TextQuery, UsageEntry, VectorQuery},
    principal::Principal,
    selection::Selection,
};
use crate::{error::ApiError, store::Store};

pub const MAX_LIMIT: u32 = 50;
const MAX_QUERY: usize = 500;
const MAX_EXCLUDED: usize = 500;

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchMemoriesBody {
    /// Texto livre (até 500 caracteres); acentos e caixa são ignorados.
    #[schema(example = "testes locais")]
    pub query: String,
    /// Slugs e prefixos (`eng/*`); restringe dentro do alcance e do `Engramind-Channels`.
    /// Canais `manage` e arquivados só entram citados pelo slug exato.
    #[schema(example = json!(["eng/global"]))]
    pub channels: Option<Vec<String>>,
    #[schema(example = json!(["CONSTRAINTS"]))]
    pub categories: Option<Vec<String>>,
    /// De 1 a 50 (padrão `search.default_limit`).
    #[schema(minimum = 1, maximum = 50)]
    pub limit: Option<u32>,
    /// Inclui as memórias arquivadas.
    pub include_archived: Option<bool>,
    /// Ids a deixar de fora (por exemplo, os que já estão no contexto).
    pub exclude_ids: Option<Vec<String>>,
    /// Exclui as memórias com texto completo no contexto do mesmo token e seleção (faixa 1
    /// de `get_context` sem `format`); somado a `exclude_ids`.
    pub exclude_context: Option<bool>,
    pub fields: Option<Fields>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SearchHit {
    /// Posição, a partir de 1.
    #[schema(example = 1)]
    pub rank: u32,
    /// `Σ peso / (search.rrf_k + posição)` sobre os dois ramos, com 6 casas.
    #[schema(example = 0.032787)]
    pub score: f64,
    pub memory: MemoryItem,
}

/// Ramos que rodaram: `hybrid` quando a consulta foi vetorizada (mesmo sem candidatos
/// vetoriais), `text` quando não (desligado, sem configuração, falha ou prazo do provedor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Hybrid,
    Text,
}

/// Pesos e `k` da fusão.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fusion {
    pub vector_weight: f64,
    pub text_weight: f64,
    pub k: f64,
}

/// RRF ponderado: `score(id) = Σ peso / (k + posição)`, ausente num ramo = 0; ordem por
/// `score` decrescente e `id` crescente; até `limit`.
pub fn fuse(
    vector: Vec<Memory>,
    text: Vec<Memory>,
    fusion: Fusion,
    limit: usize,
) -> Vec<(Memory, f64)> {
    let mut scored: Vec<(Memory, f64)> = Vec::new();
    for (branch, weight) in [(vector, fusion.vector_weight), (text, fusion.text_weight)] {
        for (index, memory) in branch.into_iter().enumerate() {
            let score = weight / (fusion.k + index as f64 + 1.0);
            match scored
                .iter_mut()
                .find(|(existing, _)| existing.id == memory.id)
            {
                Some((_, total)) => *total += score,
                None => scored.push((memory, score)),
            }
        }
    }
    scored.sort_by(|(a, x), (b, y)| y.total_cmp(x).then_with(|| a.id.cmp(&b.id)));
    scored.truncate(limit);
    scored
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SearchResults {
    pub mode: SearchMode,
    /// Da mais relevante à menos relevante.
    pub results: Vec<SearchHit>,
    /// `version` de `get_context` sem `format` para o mesmo token e seleção.
    #[schema(example = "cv_3f9e1a7c0b2d")]
    pub context_version: String,
}

impl<S: Store> Core<S> {
    pub async fn search_memories(
        &self,
        principal: &Principal,
        body: SearchMemoriesBody,
        header: Option<&str>,
    ) -> Result<SearchResults, ApiError> {
        self.authorize(principal, "search_memories")?;
        let query = body.query.trim();
        if query.chars().count() > MAX_QUERY {
            return Err(ApiError::validation(format!(
                "query deve ter ate {MAX_QUERY} caracteres"
            )));
        }
        let terms = query_terms(query);
        if terms.is_empty() {
            return Err(ApiError::validation(
                "query precisa de ao menos uma palavra",
            ));
        }
        let snapshot = self.settings_snapshot();
        let limit = body
            .limit
            .unwrap_or_else(|| u32::try_from(snapshot.whole(SEARCH_DEFAULT_LIMIT)).unwrap_or(10));
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(ApiError::validation(format!(
                "limit deve estar entre 1 e {MAX_LIMIT}"
            )));
        }
        let mut exclude_ids = body.exclude_ids.unwrap_or_default();
        if exclude_ids.len() > MAX_EXCLUDED {
            return Err(ApiError::validation(format!(
                "exclude_ids aceita ate {MAX_EXCLUDED} ids"
            )));
        }
        let channels = body.channels.map(|channels| channels.join(","));
        let selection = Selection::parse(header, channels.as_deref())?;
        let reaches = self.resolve(principal, &selection, Purpose::Search).await?;
        let (context_version, in_context) = self
            .context_for_search(principal, &selection, body.exclude_context.unwrap_or(false))
            .await?;
        exclude_ids.extend(in_context);
        let channel_ids: Vec<String> = reaches
            .iter()
            .map(|reach| reach.channel.id.clone())
            .collect();
        let mut statuses = vec![MemoryStatus::Active, MemoryStatus::Permanent];
        if body.include_archived.unwrap_or(false) {
            statuses.push(MemoryStatus::Archived);
        }
        let categories = body.categories.unwrap_or_default();
        let text_hits = self
            .store()
            .search_text(TextQuery {
                terms,
                channel_ids: channel_ids.clone(),
                categories: categories.clone(),
                statuses: statuses.clone(),
                exclude_ids: exclude_ids.clone(),
                limit,
            })
            .await?;
        let (mode, vector_hits) = match self.query_vector(query, "search_memories").await {
            Some((runtime, embedding)) => {
                let hits = self
                    .store()
                    .search_vectors(VectorQuery {
                        embedding,
                        model_id: runtime.model_id,
                        channel_ids: channel_ids.clone(),
                        categories,
                        statuses,
                        exclude_ids,
                        limit,
                    })
                    .await?;
                (
                    SearchMode::Hybrid,
                    hits.into_iter().map(|hit| hit.memory).collect(),
                )
            }
            None => (SearchMode::Text, Vec::new()),
        };
        let fusion = Fusion {
            vector_weight: snapshot.number(SEARCH_VECTOR_WEIGHT),
            text_weight: snapshot.number(SEARCH_TEXT_WEIGHT),
            k: snapshot.whole(SEARCH_RRF_K) as f64,
        };
        let found = fuse(vector_hits, text_hits, fusion, limit as usize);
        let at = ids::timestamp(ids::now());
        self.counters()
            .record(found.iter().map(|(memory, _)| memory.id.as_str()), &at);
        self.counters().record_usage(UsageEntry {
            at,
            app_id: principal.app.id.clone(),
            agent_id: principal.agent.as_ref().map(|agent| agent.id.clone()),
            token_id: principal.token_id.clone(),
            op: "search_memories",
            channel_ids,
            memory_ids: found.iter().map(|(memory, _)| memory.id.clone()).collect(),
            rendered_tokens: None,
        });
        let fields = body.fields.unwrap_or_default();
        Ok(SearchResults {
            mode,
            results: found
                .into_iter()
                .enumerate()
                .map(|(index, (memory, score))| SearchHit {
                    rank: index as u32 + 1,
                    score: round6(score),
                    memory: MemoryItem::of(memory, fields),
                })
                .collect(),
            context_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Author, EmbeddingState, TitleState};

    fn memory(id: &str) -> Memory {
        Memory {
            id: id.to_owned(),
            channel: "eng/global".to_owned(),
            channel_id: "ch_1".to_owned(),
            category: "CONSTRAINTS".to_owned(),
            title: id.to_owned(),
            title_state: TitleState::Ready,
            text: id.to_owned(),
            status: MemoryStatus::Active,
            importance: None,
            version: 1,
            embedding_state: EmbeddingState::Ready,
            author: Author {
                app_id: "app_1".to_owned(),
                app: None,
                agent_id: None,
                agent: None,
            },
            origin: None,
            retrieval_count: 0,
            injected_count: 0,
            last_retrieved_at: None,
            verified_at: None,
            created_at: String::new(),
            updated_at: String::new(),
            archived_at: None,
            stale_candidate: false,
            merged_into: None,
        }
    }

    fn ids(found: &[(Memory, f64)]) -> Vec<&str> {
        found.iter().map(|(memory, _)| memory.id.as_str()).collect()
    }

    const EQUAL: Fusion = Fusion {
        vector_weight: 1.0,
        text_weight: 1.0,
        k: 60.0,
    };

    #[test]
    fn items_in_both_branches_win_and_ties_go_by_id() {
        let found = fuse(
            vec![memory("m_b"), memory("m_c")],
            vec![memory("m_a"), memory("m_b")],
            EQUAL,
            10,
        );
        assert_eq!(ids(&found), ["m_b", "m_a", "m_c"]);
        assert_eq!(round6(found[0].1), round6(1.0 / 61.0 + 1.0 / 62.0));
        assert_eq!(round6(found[1].1), 0.016393);
    }

    #[test]
    fn weights_change_the_order_predictably() {
        let vector = || vec![memory("m_v")];
        let text = || vec![memory("m_t")];
        let only_text = Fusion {
            vector_weight: 0.0,
            ..EQUAL
        };
        assert_eq!(ids(&fuse(vector(), text(), only_text, 10)), ["m_t", "m_v"]);
        let vector_first = Fusion {
            vector_weight: 2.0,
            ..EQUAL
        };
        assert_eq!(
            ids(&fuse(vector(), text(), vector_first, 10)),
            ["m_v", "m_t"]
        );
        assert_eq!(fuse(vector(), text(), EQUAL, 1).len(), 1);
    }
}
