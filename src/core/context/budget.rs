//! Decaimento em duas faixas (desenho §8, 005 Q-001/Q-002): texto completo até
//! `context.budget_tokens`, depois `#id: título` até `context.titles_budget_tokens`, o resto no
//! excedente. A primeira memória que não cabe no orçamento global fecha a faixa para todos; a
//! que estoura o teto do próprio canal fecha a faixa só para aquele canal. Assim a ordem nunca
//! se inverte dentro de um canal nem no bloco.

use std::collections::{BTreeMap, BTreeSet};

use super::estimator::TokenEstimator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Full,
    Title,
    Overflow,
}

/// Orçamento global de uma faixa e os tetos por canal (sobreposições).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Limit {
    pub global: u32,
    pub channels: BTreeMap<String, u32>,
}

#[derive(Default)]
struct Track {
    closed: bool,
    used: u64,
    closed_channels: BTreeSet<String>,
    used_channels: BTreeMap<String, u64>,
}

impl Track {
    fn open_for(&self, channel: &str) -> bool {
        !self.closed && !self.closed_channels.contains(channel)
    }

    /// Tenta colocar `cost` tokens; fecha a faixa (global ou do canal) quando não cabe.
    fn take(&mut self, limit: &Limit, channel: &str, cost: u32) -> bool {
        let cost = u64::from(cost);
        if self.used + cost > u64::from(limit.global) {
            self.closed = true;
            return false;
        }
        let used = self.used_channels.get(channel).copied().unwrap_or(0);
        if let Some(cap) = limit.channels.get(channel)
            && used + cost > u64::from(*cap)
        {
            self.closed_channels.insert(channel.to_owned());
            return false;
        }
        self.used += cost;
        self.used_channels.insert(channel.to_owned(), used + cost);
        true
    }
}

/// Uma memória já ordenada: canal, linha completa e linha de título.
pub struct Line<'a> {
    pub channel: &'a str,
    pub full: &'a str,
    pub title: &'a str,
}

/// A faixa de cada memória, na ordem recebida.
pub fn place(
    lines: &[Line<'_>],
    full: &Limit,
    titles: &Limit,
    estimator: &dyn TokenEstimator,
) -> Vec<Band> {
    let mut full_track = Track::default();
    let mut title_track = Track::default();
    lines
        .iter()
        .map(|line| {
            if full_track.closed && title_track.closed {
                return Band::Overflow;
            }
            if full_track.open_for(line.channel)
                && full_track.take(full, line.channel, estimator.count(line.full))
            {
                return Band::Full;
            }
            if title_track.open_for(line.channel)
                && title_track.take(titles, line.channel, estimator.count(line.title))
            {
                return Band::Title;
            }
            Band::Overflow
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::estimator::Words;

    fn limit(global: u32) -> Limit {
        Limit {
            global,
            channels: BTreeMap::new(),
        }
    }

    fn line<'a>(channel: &'a str, full: &'a str) -> Line<'a> {
        Line {
            channel,
            full,
            title: "t",
        }
    }

    #[test]
    fn the_first_memory_that_does_not_fit_opens_the_titles_band() {
        let lines = [
            line("a", "um dois"),
            line("a", "um dois tres"),
            line("a", "um"),
        ];
        assert_eq!(
            place(&lines, &limit(4), &limit(10), &Words),
            [Band::Full, Band::Title, Band::Title]
        );
    }

    #[test]
    fn exact_cuts_and_overflow() {
        let lines = [
            line("a", "um dois"),
            line("a", "um dois"),
            line("a", "um"),
            line("a", "um"),
            line("a", "um"),
        ];
        assert_eq!(
            place(&lines, &limit(4), &limit(2), &Words),
            [
                Band::Full,
                Band::Full,
                Band::Title,
                Band::Title,
                Band::Overflow
            ]
        );
    }

    #[test]
    fn zero_budgets_send_everything_to_overflow() {
        let lines = [line("a", "um"), line("b", "um")];
        assert_eq!(
            place(&lines, &limit(0), &limit(0), &Words),
            [Band::Overflow, Band::Overflow]
        );
    }

    #[test]
    fn a_channel_cap_closes_the_band_only_for_that_channel() {
        let full = Limit {
            global: 10,
            channels: BTreeMap::from([("a".to_owned(), 2)]),
        };
        let lines = [
            line("a", "um dois"),
            line("a", "um"),
            line("b", "um dois tres"),
            line("a", "um"),
        ];
        assert_eq!(
            place(&lines, &full, &limit(10), &Words),
            [Band::Full, Band::Title, Band::Full, Band::Title]
        );
    }

    #[test]
    fn the_global_budget_still_applies_with_a_channel_cap() {
        let full = Limit {
            global: 3,
            channels: BTreeMap::from([("a".to_owned(), 100)]),
        };
        let lines = [line("a", "um dois"), line("b", "um dois"), line("a", "um")];
        assert_eq!(
            place(&lines, &full, &limit(0), &Words),
            [Band::Full, Band::Overflow, Band::Overflow]
        );
    }
}
