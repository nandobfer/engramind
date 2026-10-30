//! `search_memories` (desenho §9): FTS5/BM25 sem acentos sobre os canais em modo `inject` e
//! `search`, mais os `manage` citados. O ramo vetorial e a fusão completa chegam na 007; o
//! `score` já é o RRF do ramo textual, para a escala não mudar.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    Core,
    access::Purpose,
    ids,
    memories::{Fields, MemoryItem},
    memory_text::query_terms,
    model::{MemoryStatus, TextQuery},
    principal::Principal,
    selection::Selection,
};
use crate::{error::ApiError, store::Store};

pub const DEFAULT_LIMIT: u32 = 10;
pub const MAX_LIMIT: u32 = 50;
/// `k` da fusão RRF (`search.rrf_k` na 007).
pub const RRF_K: f64 = 60.0;
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
    /// De 1 a 50 (padrão 10).
    #[schema(minimum = 1, maximum = 50)]
    pub limit: Option<u32>,
    /// Inclui as memórias arquivadas.
    pub include_archived: Option<bool>,
    /// Ids a deixar de fora (por exemplo, os que já estão no contexto).
    pub exclude_ids: Option<Vec<String>>,
    pub fields: Option<Fields>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SearchHit {
    /// Posição, a partir de 1.
    #[schema(example = 1)]
    pub rank: u32,
    /// `1 / (60 + rank)`.
    #[schema(example = 0.016393)]
    pub score: f64,
    pub memory: MemoryItem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SearchResults {
    /// Da mais relevante à menos relevante.
    pub results: Vec<SearchHit>,
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
        let limit = body.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(ApiError::validation(format!(
                "limit deve estar entre 1 e {MAX_LIMIT}"
            )));
        }
        let exclude_ids = body.exclude_ids.unwrap_or_default();
        if exclude_ids.len() > MAX_EXCLUDED {
            return Err(ApiError::validation(format!(
                "exclude_ids aceita ate {MAX_EXCLUDED} ids"
            )));
        }
        let channels = body.channels.map(|channels| channels.join(","));
        let selection = Selection::parse(header, channels.as_deref())?;
        let reaches = self.resolve(principal, &selection, Purpose::Search).await?;
        let mut statuses = vec![MemoryStatus::Active, MemoryStatus::Permanent];
        if body.include_archived.unwrap_or(false) {
            statuses.push(MemoryStatus::Archived);
        }
        let found = self
            .store()
            .search_text(TextQuery {
                terms,
                channel_ids: reaches
                    .iter()
                    .map(|reach| reach.channel.id.clone())
                    .collect(),
                categories: body.categories.unwrap_or_default(),
                statuses,
                exclude_ids,
                limit,
            })
            .await?;
        let at = ids::timestamp(ids::now());
        self.counters()
            .record(found.iter().map(|memory| memory.id.as_str()), &at);
        let fields = body.fields.unwrap_or_default();
        Ok(SearchResults {
            results: found
                .into_iter()
                .enumerate()
                .map(|(index, memory)| {
                    let rank = index as u32 + 1;
                    SearchHit {
                        rank,
                        score: (1_000_000.0 / (RRF_K + f64::from(rank))).round() / 1_000_000.0,
                        memory: MemoryItem::of(memory, fields),
                    }
                })
                .collect(),
        })
    }
}
