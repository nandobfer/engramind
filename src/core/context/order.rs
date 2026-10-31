//! Ordem determinística do contexto (desenho §8, FR-002): prioridade da assinatura ou do canal
//! (maior primeiro), `permanent` primeiro, prioridade da categoria, `importance` (só quando
//! classificada; as não classificadas depois), `created_at` e `id`.

use std::cmp::{Ordering, Reverse};

use crate::core::model::{ContextMemory, MemoryStatus};

/// Uma memória com o que a ordem e o agrupamento precisam.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub memory: &'a ContextMemory,
    pub slug: &'a str,
    pub channel_priority: i64,
    pub category_priority: i64,
}

fn importance(entry: &Entry<'_>) -> (bool, f64) {
    match entry.memory.importance {
        Some(value) if value.is_finite() => (false, -value),
        _ => (true, 0.0),
    }
}

pub fn compare(a: &Entry<'_>, b: &Entry<'_>) -> Ordering {
    let permanent = |entry: &Entry<'_>| entry.memory.status != MemoryStatus::Permanent;
    Reverse(a.channel_priority)
        .cmp(&Reverse(b.channel_priority))
        .then_with(|| permanent(a).cmp(&permanent(b)))
        .then_with(|| Reverse(a.category_priority).cmp(&Reverse(b.category_priority)))
        .then_with(|| {
            let (a_missing, a_value) = importance(a);
            let (b_missing, b_value) = importance(b);
            a_missing
                .cmp(&b_missing)
                .then_with(|| a_value.total_cmp(&b_value))
        })
        .then_with(|| a.memory.created_at.cmp(&b.memory.created_at))
        .then_with(|| a.memory.id.cmp(&b.memory.id))
}

pub fn sort(entries: &mut [Entry<'_>]) {
    entries.sort_by(compare);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(id: &str) -> ContextMemory {
        ContextMemory {
            id: id.to_owned(),
            channel_id: "ch_1".to_owned(),
            category: "NAMING".to_owned(),
            title: id.to_owned(),
            text: id.to_owned(),
            status: MemoryStatus::Active,
            importance: None,
            created_at: "2026-10-30T12:00:00.000Z".to_owned(),
        }
    }

    fn entry(memory: &ContextMemory) -> Entry<'_> {
        Entry {
            memory,
            slug: "eng/global",
            channel_priority: 0,
            category_priority: 0,
        }
    }

    fn ids(entries: &[Entry<'_>]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| entry.memory.id.clone())
            .collect()
    }

    #[test]
    fn channel_priority_comes_first() {
        let (a, b) = (memory("m_a"), memory("m_b"));
        let mut entries = vec![
            Entry {
                channel_priority: 1,
                ..entry(&a)
            },
            Entry {
                channel_priority: 5,
                ..entry(&b)
            },
        ];
        sort(&mut entries);
        assert_eq!(ids(&entries), ["m_b", "m_a"]);
    }

    #[test]
    fn permanent_comes_before_a_higher_category() {
        let a = memory("m_a");
        let b = ContextMemory {
            status: MemoryStatus::Permanent,
            ..memory("m_b")
        };
        let mut entries = vec![
            Entry {
                category_priority: 50,
                ..entry(&a)
            },
            Entry {
                category_priority: 10,
                ..entry(&b)
            },
        ];
        sort(&mut entries);
        assert_eq!(ids(&entries), ["m_b", "m_a"]);
    }

    #[test]
    fn category_priority_orders_within_the_same_status() {
        let (a, b) = (memory("m_a"), memory("m_b"));
        let mut entries = vec![
            Entry {
                category_priority: 10,
                ..entry(&a)
            },
            Entry {
                category_priority: 40,
                ..entry(&b)
            },
        ];
        sort(&mut entries);
        assert_eq!(ids(&entries), ["m_b", "m_a"]);
    }

    #[test]
    fn classified_importance_goes_first_and_unclassified_last() {
        let a = memory("m_a");
        let b = ContextMemory {
            importance: Some(0.2),
            ..memory("m_b")
        };
        let c = ContextMemory {
            importance: Some(0.9),
            ..memory("m_c")
        };
        let mut entries = vec![entry(&a), entry(&b), entry(&c)];
        sort(&mut entries);
        assert_eq!(ids(&entries), ["m_c", "m_b", "m_a"]);
    }

    #[test]
    fn older_first_then_id() {
        let a = ContextMemory {
            created_at: "2026-10-30T12:00:01.000Z".to_owned(),
            ..memory("m_a")
        };
        let (b, c) = (memory("m_c"), memory("m_b"));
        let mut entries = vec![entry(&a), entry(&b), entry(&c)];
        sort(&mut entries);
        assert_eq!(ids(&entries), ["m_b", "m_c", "m_a"]);
    }
}
