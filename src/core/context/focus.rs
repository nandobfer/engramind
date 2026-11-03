//! `focus` de `get_context` (0.11.0, decisão 0029): relevância híbrida sem LLM. É relevante a
//! memória com cosseno ≥ `context.focus_min_score` no ramo vetorial ou com algum termo casado
//! no FTS5 (até `CANDIDATES` por ramo); entre as relevantes, a ordem é a fusão RRF da busca.
//! Falha do embedding → só FTS (`text`); falha do FTS → sem foco (`none`). O contexto nunca
//! derruba o turno por causa do foco.

use std::{collections::HashMap, time::Duration};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::FocusMode;
use crate::{
    core::{
        Core,
        memory_text::query_terms,
        model::{MemoryStatus, TextQuery, VectorQuery},
        search::{Fusion, fuse},
        settings::{
            FOCUS_EMBED_TIMEOUT_MS, FOCUS_MIN_SCORE, SEARCH_RRF_K, SEARCH_TEXT_WEIGHT,
            SEARCH_VECTOR_WEIGHT, SettingsSnapshot,
        },
    },
    error::ApiError,
    store::Store,
};

/// Caracteres do foco depois da normalização.
pub const MAX_FOCUS: usize = 2000;
/// Candidatos de cada ramo.
const CANDIDATES: u32 = 50;

/// `trim` e espaços juntados; vazio → sem foco; acima de `MAX_FOCUS` → `400 FOCUS_TOO_LONG`.
pub fn normalize(value: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Ok(None);
    }
    let length = normalized.chars().count();
    if length > MAX_FOCUS {
        return Err(ApiError::new(
            "FOCUS_TOO_LONG",
            format!("focus deve ter ate {MAX_FOCUS} caracteres (tem {length})"),
        )
        .with_details(json!({ "max": MAX_FOCUS, "length": length })));
    }
    Ok(Some(normalized))
}

/// O que o foco acrescenta ao hash da `version`: o digest do texto (nunca o texto), o modelo
/// vigente, o limiar e o modo.
pub fn fingerprint_entry(focus: &str, model_id: &str, min_score: f64, mode: FocusMode) -> Value {
    let digest: String = Sha256::digest(focus.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({
        "digest": digest,
        "model_id": model_id,
        "min_score": min_score,
        "mode": mode.as_str(),
    })
}

/// Relevância calculada para um foco.
pub struct Relevance {
    /// Posição (0 = a mais relevante) das memórias relevantes.
    pub ranks: Option<HashMap<String, usize>>,
    pub mode: FocusMode,
}

impl<S: Store> Core<S> {
    /// O modo esperado antes de rodar (decide a `version` do `304`) e o `model_id` vigente.
    pub(crate) async fn focus_expectation(&self) -> (FocusMode, String) {
        match self.current_runtime().await {
            Some(runtime) => (FocusMode::Hybrid, runtime.model_id),
            None => (FocusMode::Text, String::new()),
        }
    }

    /// Posições de relevância nos canais dados, com o modo que de fato rodou.
    pub(crate) async fn focus_relevance(
        &self,
        focus: &str,
        channel_ids: Vec<String>,
        snapshot: &SettingsSnapshot,
        expected: FocusMode,
    ) -> Relevance {
        let statuses = vec![MemoryStatus::Active, MemoryStatus::Permanent];
        let text = self
            .store()
            .search_text(TextQuery {
                terms: query_terms(focus),
                channel_ids: channel_ids.clone(),
                categories: Vec::new(),
                tags: Vec::new(),
                statuses: statuses.clone(),
                exclude_ids: Vec::new(),
                limit: CANDIDATES,
            })
            .await;
        let text = match text {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(
                    event = "context.focus_failed",
                    error = %error,
                    "busca textual do foco falhou; contexto segue sem foco"
                );
                return Relevance {
                    ranks: None,
                    mode: FocusMode::None,
                };
            }
        };
        let mut mode = FocusMode::Text;
        let mut vector = Vec::new();
        if expected == FocusMode::Hybrid {
            let timeout = Duration::from_millis(
                u64::try_from(snapshot.whole(FOCUS_EMBED_TIMEOUT_MS)).unwrap_or(1500),
            );
            if let Some((runtime, embedding)) = self
                .query_vector_within(focus, "get_context", timeout)
                .await
            {
                let threshold = snapshot.number(FOCUS_MIN_SCORE);
                match self
                    .store()
                    .search_vectors(VectorQuery {
                        embedding,
                        model_id: runtime.model_id,
                        channel_ids,
                        categories: Vec::new(),
                        tags: Vec::new(),
                        statuses,
                        exclude_ids: Vec::new(),
                        limit: CANDIDATES,
                    })
                    .await
                {
                    Ok(hits) => {
                        mode = FocusMode::Hybrid;
                        vector = hits
                            .into_iter()
                            .filter(|hit| 1.0 - hit.distance >= threshold)
                            .map(|hit| hit.memory)
                            .collect();
                    }
                    Err(error) => tracing::warn!(
                        event = "context.focus_degraded",
                        error = %error,
                        "ramo vetorial do foco falhou; contexto segue so com o FTS"
                    ),
                }
            }
        }
        let fusion = Fusion {
            vector_weight: snapshot.number(SEARCH_VECTOR_WEIGHT),
            text_weight: snapshot.number(SEARCH_TEXT_WEIGHT),
            k: snapshot.whole(SEARCH_RRF_K) as f64,
        };
        let limit = vector.len() + text.len();
        let ranks = fuse(vector, text, fusion, limit)
            .into_iter()
            .enumerate()
            .map(|(rank, (memory, _))| (memory.id, rank))
            .collect();
        Relevance {
            ranks: Some(ranks),
            mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_is_trimmed_collapsed_and_bounded() {
        assert_eq!(normalize(None).expect("none"), None);
        assert_eq!(normalize(Some("   \n\t ")).expect("blank"), None);
        assert_eq!(
            normalize(Some("  como   rodo\nos testes ")).expect("ok"),
            Some("como rodo os testes".to_owned())
        );
        let long = "a".repeat(MAX_FOCUS);
        assert_eq!(normalize(Some(&long)).expect("max"), Some(long.clone()));
        let error = normalize(Some(&format!("{long}b"))).expect_err("too long");
        assert_eq!(error.code, "FOCUS_TOO_LONG");
    }

    #[test]
    fn the_fingerprint_never_carries_the_text() {
        let entry = fingerprint_entry("segredo do turno", "m1", 0.35, FocusMode::Hybrid);
        assert!(!entry.to_string().contains("segredo"));
        assert_ne!(
            entry,
            fingerprint_entry("segredo do turno", "m1", 0.35, FocusMode::Text)
        );
        assert_ne!(
            entry,
            fingerprint_entry("outro turno", "m1", 0.35, FocusMode::Hybrid)
        );
    }
}
