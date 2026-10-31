//! Planejador puro da consolidação (009 D-004): a partir das memórias ativas do canal, dos
//! vetores do modelo vigente e dos pares já recusados, decide os candidatos de cada tarefa. O
//! `dry_run` devolve o plano e a execução age só sobre ele.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{ConsolidationMemory, DeclinedPair, Task};

/// Pares por canal por execução.
pub const MAX_PAIRS: usize = 20;
/// Memórias por chamada de importância.
pub const IMPORTANCE_BATCH: usize = 20;
/// Chamadas de importância por canal por execução.
pub const MAX_IMPORTANCE_CALLS: usize = 10;

/// Por que uma tarefa foi pulada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Redator desligado ou sem provedor, modelo ou chave.
    RedatorNotConfigured,
    /// `embedding.provider = off`.
    EmbeddingsOff,
}

/// O que o planejador precisa saber além das memórias.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanOptions {
    pub tasks: Vec<Task>,
    pub duplicate_threshold: f64,
    /// Instante de corte do desuso (RFC 3339, ordenável): último uso antes dele → candidata.
    pub stale_before: String,
    pub auto_archive_stale: bool,
    pub redator_ready: bool,
    pub embeddings_on: bool,
}

/// Par candidato: `keep` é a mais antiga (`created_at`, `id`).
#[derive(Debug, Clone, PartialEq)]
pub struct PairCandidate {
    pub keep: usize,
    pub archive: usize,
    pub similarity: f64,
}

/// Desuso: sinalizar, limpar ou arquivar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StalePlan {
    pub flag: Vec<usize>,
    pub clear: Vec<usize>,
    pub archive: Vec<usize>,
}

/// Plano de um canal; os índices apontam para `ConsolidationInput::memories`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChannelPlan {
    pub skipped: BTreeMap<Task, SkipReason>,
    pub duplicates: Vec<PairCandidate>,
    /// Lotes de até `IMPORTANCE_BATCH`.
    pub classify: Vec<Vec<usize>>,
    pub stale: StalePlan,
}

/// Último sinal de uso: edição, recuperação, injeção ou conferência (009 Q-003).
pub fn last_use(memory: &ConsolidationMemory) -> &str {
    [
        Some(memory.updated_at.as_str()),
        memory.last_retrieved_at.as_deref(),
        memory.last_injected_at.as_deref(),
        memory.verified_at.as_deref(),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or("")
}

fn normalized(vector: &[f32]) -> Option<Vec<f32>> {
    let norm = vector
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return None;
    }
    Some(
        vector
            .iter()
            .map(|value| (f64::from(*value) / norm) as f32)
            .collect(),
    )
}

/// Pares com cosseno `>= threshold`, do maior para o menor (desempate pelos índices). Vetores
/// de dimensões diferentes ou nulos nunca formam par. Varredura exata (009 D-005).
pub fn cosine_pairs(vectors: &[Option<&[f32]>], threshold: f64) -> Vec<(usize, usize, f64)> {
    let normalized: Vec<Option<Vec<f32>>> = vectors
        .iter()
        .map(|vector| vector.and_then(normalized))
        .collect();
    let mut pairs = Vec::new();
    for (a, left) in normalized.iter().enumerate() {
        let Some(left) = left else { continue };
        for (offset, right) in normalized[a + 1..].iter().enumerate() {
            let Some(right) = right else { continue };
            if left.len() != right.len() {
                continue;
            }
            let cosine: f64 = left
                .iter()
                .zip(right)
                .map(|(x, y)| f64::from(*x) * f64::from(*y))
                .sum();
            if cosine >= threshold {
                pairs.push((a, a + 1 + offset, cosine));
            }
        }
    }
    pairs.sort_by(|x, y| {
        y.2.total_cmp(&x.2)
            .then_with(|| x.0.cmp(&y.0))
            .then_with(|| x.1.cmp(&y.1))
    });
    pairs
}

fn older(memories: &[ConsolidationMemory], a: usize, b: usize) -> (usize, usize) {
    let key = |index: usize| (&memories[index].created_at, &memories[index].id);
    if key(a) <= key(b) { (a, b) } else { (b, a) }
}

/// Monta o plano. `memories` são as `active` do canal, em qualquer ordem.
pub fn plan(
    memories: &[ConsolidationMemory],
    declined: &[DeclinedPair],
    options: &PlanOptions,
) -> ChannelPlan {
    let mut plan = ChannelPlan::default();
    let wants = |task: Task| options.tasks.contains(&task);
    let mut paired = BTreeSet::new();

    if wants(Task::Duplicates) {
        if !options.redator_ready {
            plan.skipped
                .insert(Task::Duplicates, SkipReason::RedatorNotConfigured);
        } else if !options.embeddings_on {
            plan.skipped
                .insert(Task::Duplicates, SkipReason::EmbeddingsOff);
        } else {
            let refused: BTreeSet<(&str, &[u8], &str, &[u8])> = declined
                .iter()
                .map(|pair| {
                    (
                        pair.memory_a.as_str(),
                        pair.hash_a.as_slice(),
                        pair.memory_b.as_str(),
                        pair.hash_b.as_slice(),
                    )
                })
                .collect();
            let vectors: Vec<Option<&[f32]>> = memories
                .iter()
                .map(|memory| memory.vector.as_deref())
                .collect();
            for (a, b, similarity) in cosine_pairs(&vectors, options.duplicate_threshold) {
                if plan.duplicates.len() >= MAX_PAIRS {
                    break;
                }
                if paired.contains(&a) || paired.contains(&b) {
                    continue;
                }
                let (first, second) = if memories[a].id < memories[b].id {
                    (a, b)
                } else {
                    (b, a)
                };
                if refused.contains(&(
                    memories[first].id.as_str(),
                    memories[first].content_hash.as_slice(),
                    memories[second].id.as_str(),
                    memories[second].content_hash.as_slice(),
                )) {
                    continue;
                }
                // Mesmo texto (hash igual) não acontece com a deduplicação exata; ainda assim
                // o Redator decide.
                let (keep, archive) = older(memories, a, b);
                paired.insert(a);
                paired.insert(b);
                plan.duplicates.push(PairCandidate {
                    keep,
                    archive,
                    similarity: (similarity * 1000.0).round() / 1000.0,
                });
            }
        }
    }

    if wants(Task::Importance) {
        if options.redator_ready {
            let mut pending: Vec<usize> = (0..memories.len())
                .filter(|index| !paired.contains(index))
                .filter(|index| {
                    memories[*index].importance.is_none()
                        && memories[*index].importance_pending.is_none()
                })
                .collect();
            pending.sort_by(|a, b| {
                (&memories[*a].created_at, &memories[*a].id)
                    .cmp(&(&memories[*b].created_at, &memories[*b].id))
            });
            pending.truncate(IMPORTANCE_BATCH * MAX_IMPORTANCE_CALLS);
            plan.classify = pending
                .chunks(IMPORTANCE_BATCH)
                .map(<[usize]>::to_vec)
                .collect();
        } else {
            plan.skipped
                .insert(Task::Importance, SkipReason::RedatorNotConfigured);
        }
    }

    if wants(Task::Stale) {
        let mut order: Vec<usize> = (0..memories.len())
            .filter(|index| !paired.contains(index))
            .collect();
        order.sort_by(|a, b| memories[*a].id.cmp(&memories[*b].id));
        for index in order {
            let memory = &memories[index];
            let stale = last_use(memory) < options.stale_before.as_str();
            match (stale, memory.stale_candidate) {
                (true, _) if options.auto_archive_stale => plan.stale.archive.push(index),
                (true, false) => plan.stale.flag.push(index),
                (false, true) => plan.stale.clear.push(index),
                _ => {}
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(id: &str, created_at: &str, vector: Option<Vec<f32>>) -> ConsolidationMemory {
        ConsolidationMemory {
            id: id.to_owned(),
            category: "NAMING".to_owned(),
            title: id.to_owned(),
            text: format!("texto {id}"),
            content_hash: id.as_bytes().to_vec(),
            created_at: created_at.to_owned(),
            updated_at: created_at.to_owned(),
            importance: None,
            importance_pending: None,
            last_retrieved_at: None,
            last_injected_at: None,
            verified_at: None,
            stale_candidate: false,
            vector,
        }
    }

    fn options(tasks: &[Task]) -> PlanOptions {
        PlanOptions {
            tasks: tasks.to_vec(),
            duplicate_threshold: 0.9,
            stale_before: "2026-08-01T00:00:00.000Z".to_owned(),
            auto_archive_stale: false,
            redator_ready: true,
            embeddings_on: true,
        }
    }

    #[test]
    fn cosine_pairs_are_sorted_and_respect_the_threshold() {
        let a = [1.0, 0.0];
        let b = [0.99, 0.1];
        let c = [0.0, 1.0];
        let d = [2.0, 0.0];
        let vectors: Vec<Option<&[f32]>> = vec![Some(&a), Some(&b), Some(&c), Some(&d), None];
        let pairs = cosine_pairs(&vectors, 0.9);
        let indexes: Vec<(usize, usize)> = pairs.iter().map(|pair| (pair.0, pair.1)).collect();
        assert_eq!(indexes, vec![(0, 3), (0, 1), (1, 3)]);
        assert!((pairs[0].2 - 1.0).abs() < 1e-6);
        assert!(cosine_pairs(&[Some(&[0.0, 0.0][..]), Some(&a)], 0.0).is_empty());
        assert!(cosine_pairs(&[Some(&[1.0, 0.0, 0.0][..]), Some(&a)], 0.0).is_empty());
    }

    #[test]
    fn each_memory_enters_one_pair_and_the_older_is_kept() {
        let memories = vec![
            memory("m_3", "2026-10-03T00:00:00.000Z", Some(vec![1.0, 0.0])),
            memory("m_1", "2026-10-01T00:00:00.000Z", Some(vec![1.0, 0.001])),
            memory("m_2", "2026-10-02T00:00:00.000Z", Some(vec![1.0, 0.3])),
            memory("m_4", "2026-10-04T00:00:00.000Z", None),
        ];
        let plan = plan(&memories, &[], &options(&[Task::Duplicates]));
        assert_eq!(plan.duplicates.len(), 1);
        let pair = &plan.duplicates[0];
        assert_eq!(memories[pair.keep].id, "m_1");
        assert_eq!(memories[pair.archive].id, "m_3");
        assert!(pair.similarity > 0.99);
    }

    #[test]
    fn declined_pairs_are_skipped_until_a_text_changes() {
        let memories = vec![
            memory("m_1", "2026-10-01T00:00:00.000Z", Some(vec![1.0, 0.0])),
            memory("m_2", "2026-10-02T00:00:00.000Z", Some(vec![1.0, 0.0])),
        ];
        let declined = vec![DeclinedPair {
            memory_a: "m_1".to_owned(),
            memory_b: "m_2".to_owned(),
            hash_a: b"m_1".to_vec(),
            hash_b: b"m_2".to_vec(),
        }];
        let tasks = options(&[Task::Duplicates]);
        assert!(plan(&memories, &declined, &tasks).duplicates.is_empty());
        let mut changed = memories.clone();
        changed[1].content_hash = b"outro".to_vec();
        let found = plan(&changed, &declined, &tasks).duplicates;
        assert_eq!(found.len(), 1);
        assert_eq!(changed[found[0].keep].id, "m_1");
        assert_eq!(changed[found[0].archive].id, "m_2");
    }

    #[test]
    fn pairs_are_capped_per_run() {
        let memories: Vec<ConsolidationMemory> = (0..60)
            .map(|index| {
                memory(
                    &format!("m_{index:02}"),
                    "2026-10-01T00:00:00.000Z",
                    Some(vec![1.0, index as f32 * 1e-4]),
                )
            })
            .collect();
        let plan = plan(&memories, &[], &options(&[Task::Duplicates]));
        assert_eq!(plan.duplicates.len(), MAX_PAIRS);
    }

    #[test]
    fn importance_takes_unclassified_in_batches_outside_the_pairs() {
        let mut memories: Vec<ConsolidationMemory> = (0..250)
            .map(|index| memory(&format!("m_{index:03}"), "2026-10-01T00:00:00.000Z", None))
            .collect();
        memories[0].importance = Some(0.5);
        memories[1].importance_pending = Some(0.5);
        let plan = plan(&memories, &[], &options(&[Task::Importance]));
        assert_eq!(plan.classify.len(), MAX_IMPORTANCE_CALLS);
        assert!(
            plan.classify
                .iter()
                .all(|batch| batch.len() == IMPORTANCE_BATCH)
        );
        assert_eq!(plan.classify[0][0], 2);
    }

    #[test]
    fn tasks_without_providers_are_skipped() {
        let memories = vec![memory("m_1", "2026-10-01T00:00:00.000Z", None)];
        let mut tasks = options(&[Task::Duplicates, Task::Importance, Task::Stale]);
        tasks.redator_ready = false;
        let found = plan(&memories, &[], &tasks);
        assert_eq!(
            found.skipped.get(&Task::Duplicates),
            Some(&SkipReason::RedatorNotConfigured)
        );
        assert_eq!(
            found.skipped.get(&Task::Importance),
            Some(&SkipReason::RedatorNotConfigured)
        );
        tasks.redator_ready = true;
        tasks.embeddings_on = false;
        let found = plan(&memories, &[], &tasks);
        assert_eq!(
            found.skipped.get(&Task::Duplicates),
            Some(&SkipReason::EmbeddingsOff)
        );
        assert!(!found.skipped.contains_key(&Task::Importance));
    }

    #[test]
    fn stale_uses_every_signal_and_clears_in_both_directions() {
        let old = "2026-01-01T00:00:00.000Z";
        let recent = "2026-10-01T00:00:00.000Z";
        let mut memories: Vec<ConsolidationMemory> = (0..6)
            .map(|index| memory(&format!("m_{index}"), old, None))
            .collect();
        memories[1].last_retrieved_at = Some(recent.to_owned());
        memories[2].last_injected_at = Some(recent.to_owned());
        memories[3].verified_at = Some(recent.to_owned());
        memories[4].updated_at = recent.to_owned();
        memories[4].stale_candidate = true;
        memories[5].stale_candidate = true;
        let tasks = options(&[Task::Stale]);
        let found = plan(&memories, &[], &tasks).stale;
        assert_eq!(found.flag, vec![0]);
        assert_eq!(found.clear, vec![4]);
        assert!(found.archive.is_empty());
        let mut archive = tasks.clone();
        archive.auto_archive_stale = true;
        let found = plan(&memories, &[], &archive).stale;
        assert_eq!(found.archive, vec![0, 5]);
        assert!(found.flag.is_empty());
    }
}
