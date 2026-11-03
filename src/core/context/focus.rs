//! `focus` de `get_context` (0.11.0, decisão 0029): relevância híbrida sem LLM. É relevante a
//! memória com cosseno ≥ `context.focus_min_score` no ramo vetorial ou com algum termo casado
//! no FTS5 (até `CANDIDATES` por ramo **em cada canal**, para que um canal com muitos acertos não
//! tire os candidatos dos outros). Os candidatos de todos os canais são ordenados juntos (cosseno
//! e BM25 globais) e fundidos numa fusão RRF só, então a posição é comparável entre canais da
//! mesma prioridade.
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
/// Candidatos de cada ramo, por canal.
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
/// vigente, o limiar, os pesos da fusão RRF (`search.*`, que decidem a ordem das relevantes) e o
/// modo.
pub fn fingerprint_entry(
    focus: &str,
    model_id: &str,
    snapshot: &SettingsSnapshot,
    mode: FocusMode,
) -> Value {
    let digest: String = Sha256::digest(focus.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({
        "digest": digest,
        "model_id": model_id,
        "min_score": snapshot.number(FOCUS_MIN_SCORE),
        "vector_weight": snapshot.number(SEARCH_VECTOR_WEIGHT),
        "text_weight": snapshot.number(SEARCH_TEXT_WEIGHT),
        "rrf_k": snapshot.whole(SEARCH_RRF_K),
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

    /// Posições de relevância nos canais dados, com o modo que de fato rodou. Os candidatos são
    /// buscados canal a canal e ranqueados juntos.
    pub(crate) async fn focus_relevance(
        &self,
        focus: &str,
        channel_ids: Vec<String>,
        snapshot: &SettingsSnapshot,
        expected: FocusMode,
    ) -> Relevance {
        let statuses = vec![MemoryStatus::Active, MemoryStatus::Permanent];
        let terms = query_terms(focus);
        let text_query = |channel_ids: Vec<String>, only_ids: Vec<String>, limit: u32| TextQuery {
            terms: terms.clone(),
            channel_ids,
            categories: Vec::new(),
            tags: Vec::new(),
            statuses: statuses.clone(),
            exclude_ids: Vec::new(),
            only_ids,
            limit,
        };
        let failed = |error: crate::store::StoreError| {
            tracing::warn!(
                event = "context.focus_failed",
                error = %error,
                "busca textual do foco falhou; contexto segue sem foco"
            );
            Relevance {
                ranks: None,
                mode: FocusMode::None,
            }
        };
        let mut candidates = Vec::new();
        for channel_id in &channel_ids {
            match self
                .store()
                .search_text(text_query(vec![channel_id.clone()], Vec::new(), CANDIDATES))
                .await
            {
                Ok(found) => candidates.extend(found.into_iter().map(|memory| memory.id)),
                Err(error) => return failed(error),
            }
        }
        // Uma busca só sobre os candidatos de todos os canais: a ordem BM25 fica global.
        let text = if candidates.is_empty() {
            Vec::new()
        } else {
            let limit = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
            match self
                .store()
                .search_text(text_query(channel_ids.clone(), candidates, limit))
                .await
            {
                Ok(text) => text,
                Err(error) => return failed(error),
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
                let mut hits = Vec::new();
                let mut ok = true;
                for channel_id in &channel_ids {
                    match self
                        .store()
                        .search_vectors(VectorQuery {
                            embedding: embedding.clone(),
                            model_id: runtime.model_id.clone(),
                            channel_ids: vec![channel_id.clone()],
                            categories: Vec::new(),
                            tags: Vec::new(),
                            statuses: statuses.clone(),
                            exclude_ids: Vec::new(),
                            limit: CANDIDATES,
                        })
                        .await
                    {
                        Ok(found) => hits.extend(
                            found
                                .into_iter()
                                .filter(|hit| 1.0 - hit.distance >= threshold),
                        ),
                        Err(error) => {
                            tracing::warn!(
                                event = "context.focus_degraded",
                                error = %error,
                                "ramo vetorial do foco falhou; contexto segue so com o FTS"
                            );
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    mode = FocusMode::Hybrid;
                    // O cosseno é comparável entre canais: uma ordem só.
                    hits.sort_by(|a, b| {
                        a.distance
                            .total_cmp(&b.distance)
                            .then_with(|| a.memory.id.cmp(&b.memory.id))
                    });
                    vector = hits.into_iter().map(|hit| hit.memory).collect();
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
        let snapshot = SettingsSnapshot::default();
        let entry = fingerprint_entry("segredo do turno", "m1", &snapshot, FocusMode::Hybrid);
        assert!(!entry.to_string().contains("segredo"));
        assert_ne!(
            entry,
            fingerprint_entry("segredo do turno", "m1", &snapshot, FocusMode::Text)
        );
        assert_ne!(
            entry,
            fingerprint_entry("outro turno", "m1", &snapshot, FocusMode::Hybrid)
        );
    }

    /// Os pesos da fusão decidem a ordem das relevantes: mudar um muda a `version` com foco.
    #[test]
    fn the_fusion_settings_enter_the_fingerprint() {
        let base = SettingsSnapshot::default();
        let entry = fingerprint_entry("turno", "m1", &base, FocusMode::Hybrid);
        for (key, value) in [
            (FOCUS_MIN_SCORE, "0.5"),
            (SEARCH_VECTOR_WEIGHT, "2"),
            (SEARCH_TEXT_WEIGHT, "0.5"),
            (SEARCH_RRF_K, "10"),
        ] {
            let changed = SettingsSnapshot::from_rows(&[crate::core::model::SettingRow {
                key: key.to_owned(),
                value_json: value.to_owned(),
                updated_by_app: None,
                updated_at: String::new(),
            }]);
            assert_ne!(
                entry,
                fingerprint_entry("turno", "m1", &changed, FocusMode::Hybrid),
                "{key}"
            );
        }
    }
}
