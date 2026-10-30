//! `retrieval_count` e `last_retrieved_at` acumulados em memória e gravados em lote a cada 5 s
//! (desenho §6, D-004), sem mudar `version` nem a ordem do contexto.

use std::{collections::HashMap, sync::Mutex};

use super::{Core, model::RetrievalBump};
use crate::store::{Store, StoreError};

#[derive(Default)]
pub struct Counters {
    pending: Mutex<HashMap<String, (i64, String)>>,
}

impl Counters {
    /// Conta uma recuperação de cada memória no instante `at`.
    pub fn record<'a>(&self, ids: impl IntoIterator<Item = &'a str>, at: &str) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        for id in ids {
            let entry = pending
                .entry(id.to_owned())
                .or_insert_with(|| (0, at.to_owned()));
            entry.0 += 1;
            if entry.1.as_str() < at {
                entry.1 = at.to_owned();
            }
        }
    }

    /// Retira os contadores acumulados para a gravação.
    pub fn drain(&self) -> Vec<RetrievalBump> {
        let Ok(mut pending) = self.pending.lock() else {
            return Vec::new();
        };
        pending
            .drain()
            .map(|(memory_id, (count, at))| RetrievalBump {
                memory_id,
                count,
                at,
            })
            .collect()
    }
}

impl<S: Store> Core<S> {
    /// Grava os contadores acumulados.
    pub async fn flush_counters(&self) -> Result<(), StoreError> {
        let batch = self.counters().drain();
        self.store().bump_retrievals(batch).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_accumulate_and_keep_the_latest_instant() {
        let counters = Counters::default();
        counters.record(["m_1", "m_2"], "2026-10-30T12:00:00.000Z");
        counters.record(["m_1"], "2026-10-30T12:00:05.000Z");
        counters.record(["m_1"], "2026-10-30T11:00:00.000Z");
        let mut batch = counters.drain();
        batch.sort_by(|a, b| a.memory_id.cmp(&b.memory_id));
        assert_eq!(batch[0].count, 3);
        assert_eq!(batch[0].at, "2026-10-30T12:00:05.000Z");
        assert_eq!(batch[1].count, 1);
        assert!(counters.drain().is_empty());
    }
}
