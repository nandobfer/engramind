//! Rate limit por token (janela fixa de 60 s) e `last_used_at` acumulado em memória (D-007).
//! Há uma única instância por banco, então o estado local é o estado do serviço.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Usage {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    windows: HashMap<String, Window>,
    last_used: HashMap<String, String>,
}

struct Window {
    started: Instant,
    count: u32,
}

impl Usage {
    /// Conta uma requisição do token. `Err(segundos)` quando o limite da janela foi atingido.
    pub fn hit(&self, token_id: &str, limit: u32, now: Instant, at: String) -> Result<(), u64> {
        let Ok(mut state) = self.state.lock() else {
            return Ok(());
        };
        state.last_used.insert(token_id.to_owned(), at);
        let window = state.windows.entry(token_id.to_owned()).or_insert(Window {
            started: now,
            count: 0,
        });
        let elapsed = now.saturating_duration_since(window.started);
        if elapsed >= WINDOW {
            window.started = now;
            window.count = 0;
        }
        if window.count >= limit {
            let remaining = WINDOW.saturating_sub(now.saturating_duration_since(window.started));
            return Err(remaining.as_secs().max(1));
        }
        window.count += 1;
        Ok(())
    }

    /// Retira os `last_used_at` acumulados para gravação em lote.
    pub fn drain_last_used(&self) -> Vec<(String, String)> {
        let Ok(mut state) = self.state.lock() else {
            return Vec::new();
        };
        let now = Instant::now();
        state
            .windows
            .retain(|_, window| now.saturating_duration_since(window.started) < WINDOW);
        state.last_used.drain().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_blocks_above_the_limit_and_resets_after_sixty_seconds() {
        let usage = Usage::default();
        let start = Instant::now();
        let at = || "2026-10-30T12:00:00.000Z".to_owned();
        assert!(usage.hit("tk_1", 2, start, at()).is_ok());
        assert!(usage.hit("tk_1", 2, start, at()).is_ok());
        let retry = usage
            .hit("tk_1", 2, start + Duration::from_secs(15), at())
            .expect_err("limited");
        assert_eq!(retry, 45);
        assert!(usage.hit("tk_2", 2, start, at()).is_ok(), "per token");
        assert!(
            usage
                .hit("tk_1", 2, start + Duration::from_secs(60), at())
                .is_ok()
        );
    }

    #[test]
    fn last_used_is_drained_once() {
        let usage = Usage::default();
        let now = Instant::now();
        usage
            .hit("tk_1", 10, now, "2026-10-30T12:00:00.000Z".to_owned())
            .expect("hit");
        usage
            .hit("tk_1", 10, now, "2026-10-30T12:00:01.000Z".to_owned())
            .expect("hit");
        assert_eq!(
            usage.drain_last_used(),
            vec![("tk_1".to_owned(), "2026-10-30T12:00:01.000Z".to_owned())]
        );
        assert!(usage.drain_last_used().is_empty());
    }
}
