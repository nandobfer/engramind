//! `retrieval_count`/`last_retrieved_at`, `injected_count` e as linhas de `usage_log`
//! acumulados em memória e gravados em lote a cada 5 s (desenho §6, 004 D-004, 005 D-004), sem
//! mudar `version` nem a ordem do contexto.

use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use super::{
    Core,
    model::{InjectionBump, RetrievalBump, UsageEntry},
};
use crate::store::{Store, StoreError};

/// Linhas de `usage_log` guardadas entre dois lotes; acima disso, as novas são descartadas.
const MAX_PENDING_USAGE: usize = 50_000;

#[derive(Default)]
pub struct Counters {
    pending: Mutex<HashMap<String, (i64, String)>>,
    injected: Mutex<HashMap<String, i64>>,
    usage: Mutex<Vec<UsageEntry>>,
    dropped_usage: AtomicU64,
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

    /// Conta uma injeção de cada memória (faixa de texto completo do contexto).
    pub fn record_injected<'a>(&self, ids: impl IntoIterator<Item = &'a str>) {
        let Ok(mut injected) = self.injected.lock() else {
            return;
        };
        for id in ids {
            *injected.entry(id.to_owned()).or_default() += 1;
        }
    }

    /// Guarda uma linha de `usage_log` para o próximo lote.
    pub fn record_usage(&self, entry: UsageEntry) {
        let Ok(mut usage) = self.usage.lock() else {
            return;
        };
        if usage.len() >= MAX_PENDING_USAGE {
            self.dropped_usage.fetch_add(1, Ordering::Relaxed);
            return;
        }
        usage.push(entry);
    }

    pub fn drain_injected(&self) -> Vec<InjectionBump> {
        let Ok(mut injected) = self.injected.lock() else {
            return Vec::new();
        };
        injected
            .drain()
            .map(|(memory_id, count)| InjectionBump { memory_id, count })
            .collect()
    }

    /// Linhas acumuladas e quantas foram descartadas desde o último lote.
    pub fn drain_usage(&self) -> (Vec<UsageEntry>, u64) {
        let Ok(mut usage) = self.usage.lock() else {
            return (Vec::new(), 0);
        };
        (
            std::mem::take(&mut *usage),
            self.dropped_usage.swap(0, Ordering::Relaxed),
        )
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
    /// Grava os contadores e o `usage_log` acumulados.
    pub async fn flush_counters(&self) -> Result<(), StoreError> {
        let counters = self.counters();
        self.store().bump_retrievals(counters.drain()).await?;
        self.store()
            .bump_injections(counters.drain_injected())
            .await?;
        let (usage, dropped) = counters.drain_usage();
        if dropped > 0 {
            tracing::warn!(
                event = "usage.dropped",
                dropped,
                "linhas de usage_log descartadas por excesso entre lotes"
            );
        }
        self.store().append_usage(usage).await
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

    #[test]
    fn injections_and_usage_are_drained_once() {
        let counters = Counters::default();
        counters.record_injected(["m_1", "m_2"]);
        counters.record_injected(["m_1"]);
        let mut injected = counters.drain_injected();
        injected.sort_by(|a, b| a.memory_id.cmp(&b.memory_id));
        assert_eq!(injected[0].count, 2);
        assert_eq!(injected[1].count, 1);
        assert!(counters.drain_injected().is_empty());
        counters.record_usage(UsageEntry {
            at: "2026-10-30T12:00:00.000Z".to_owned(),
            app_id: "app_1".to_owned(),
            agent_id: None,
            token_id: "tk_1".to_owned(),
            op: "get_context",
            channel_ids: vec!["ch_1".to_owned()],
            memory_ids: vec!["m_1".to_owned()],
            rendered_tokens: Some(12),
        });
        let (usage, dropped) = counters.drain_usage();
        assert_eq!((usage.len(), dropped), (1, 0));
        assert!(counters.drain_usage().0.is_empty());
    }
}
