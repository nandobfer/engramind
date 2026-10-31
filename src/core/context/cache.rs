//! Cache LRU dos blocos renderizados (005 D-003): chave = digest completo da `version`, então
//! tokens com o mesmo alcance compartilham a entrada e nenhuma mudança real reaproveita um
//! bloco antigo.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use super::{Rendered, version::Fingerprint};

pub const CAPACITY: usize = 1024;

pub struct ContextCache {
    capacity: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    tick: u64,
    entries: HashMap<Fingerprint, (u64, Arc<Rendered>)>,
}

impl Default for ContextCache {
    fn default() -> Self {
        Self::new(CAPACITY)
    }
}

impl ContextCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::default(),
        }
    }

    pub fn get(&self, key: &Fingerprint) -> Option<Arc<Rendered>> {
        let mut state = self.state.lock().ok()?;
        state.tick += 1;
        let tick = state.tick;
        let entry = state.entries.get_mut(key)?;
        entry.0 = tick;
        Some(entry.1.clone())
    }

    pub fn insert(&self, key: Fingerprint, rendered: Arc<Rendered>) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.tick += 1;
        let tick = state.tick;
        if !state.entries.contains_key(&key)
            && state.entries.len() >= self.capacity
            && let Some(oldest) = state
                .entries
                .iter()
                .min_by_key(|(_, (used, _))| *used)
                .map(|(key, _)| *key)
        {
            state.entries.remove(&oldest);
        }
        state.entries.insert(key, (tick, rendered));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.entries.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(tag: &str) -> Arc<Rendered> {
        Arc::new(Rendered::empty(tag))
    }

    #[test]
    fn the_least_recently_used_entry_is_evicted() {
        let cache = ContextCache::new(2);
        cache.insert([1; 32], rendered("a"));
        cache.insert([2; 32], rendered("b"));
        assert!(cache.get(&[1; 32]).is_some());
        cache.insert([3; 32], rendered("c"));
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&[2; 32]).is_none());
        assert_eq!(cache.get(&[1; 32]).expect("a").view.version, "a");
        assert_eq!(cache.get(&[3; 32]).expect("c").view.version, "c");
    }
}
