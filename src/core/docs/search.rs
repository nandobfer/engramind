//! Busca por palavras-chave nas seções dos guias e nos resumos das operações (portada do
//! sap-middleman).
//!
//! The query is split into topics. Each topic matches through its normalized
//! term, a curated Portuguese/English synonym, or, for composite terms such as
//! `idempotency-key`, all of its parts. Results are ranked by how many query
//! topics they cover, then by topics matched in the heading, then by
//! occurrences.

use super::catalog::{normalize, normalize_chars};

pub const MAX_SEARCH_TERMS: usize = 10;
const MAX_OCCURRENCES_PER_TOPIC: usize = 10;
const SNIPPET_CHARS: usize = 280;

/// Curated equivalences between the English vocabulary agents often use and
/// the Portuguese vocabulary of the guides. Values are already normalized.
/// Matching is by substring, so plural and inflected forms need no entry.
const SYNONYMS: &[&[&str]] = &[
    &["idempoten"],
    &["retry", "retries", "retentativa", "reenvio"],
    &["error", "erro"],
    &["memory", "memories", "memoria"],
    &["channel", "canal", "canais"],
    &["subscription", "assinatura"],
    &["agent", "agente"],
    &["scope", "escopo"],
    &["token"],
    &["revoke", "revoga"],
    &["issue", "emiss", "emitir", "emite"],
    &["setting", "configurac"],
    &["audit", "auditoria"],
    &["limit", "limite"],
    &["guide", "guia"],
    &["search", "busca", "pesquisa"],
    &["authentication", "autenticacao"],
    &["authorization", "autorizacao"],
    &["pagination", "paginacao"],
    &["status", "estado"],
    &["validation", "validacao"],
    &["date", "data"],
];

/// One query term with every accepted spelling.
#[derive(Debug, PartialEq, Eq)]
pub struct Topic {
    pub term: String,
    /// The term itself and its synonyms.
    variants: Vec<String>,
    /// For composite terms, each part with its synonyms; all parts must match.
    parts: Vec<Vec<String>>,
}

impl Topic {
    fn new(term: &str) -> Self {
        let parts: Vec<Vec<String>> = if term.contains(['-', '_']) {
            term.split(['-', '_'])
                .filter(|part| part.chars().count() >= 2)
                .map(variants_of)
                .collect()
        } else {
            Vec::new()
        };
        Self {
            term: term.to_owned(),
            variants: variants_of(term),
            parts: if parts.len() > 1 { parts } else { Vec::new() },
        }
    }

    /// Character positions of the first match of each occurrence in `text`.
    fn matches(&self, normalized: &str) -> Vec<usize> {
        let whole = positions(normalized, &self.variants);
        if !whole.is_empty() || self.parts.is_empty() {
            return whole;
        }
        let mut combined = Vec::new();
        for part in &self.parts {
            let found = positions(normalized, part);
            if found.is_empty() {
                return Vec::new();
            }
            combined.extend(found);
        }
        combined.sort_unstable();
        combined
    }
}

fn variants_of(term: &str) -> Vec<String> {
    let mut variants = vec![term.to_owned()];
    for group in SYNONYMS {
        // Short prefixes such as `da` must not pull unrelated synonym groups.
        if group.iter().any(|synonym| {
            term.contains(synonym) || (term.chars().count() >= 4 && synonym.starts_with(term))
        }) {
            for synonym in *group {
                if !variants.iter().any(|variant| variant == synonym) {
                    variants.push((*synonym).to_owned());
                }
            }
        }
    }
    variants
}

fn positions(normalized: &str, variants: &[String]) -> Vec<usize> {
    let mut found: Vec<usize> = variants
        .iter()
        .flat_map(|variant| {
            normalized
                .match_indices(variant.as_str())
                .map(|(byte, _)| normalized[..byte].chars().count())
        })
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

/// Splits the query into at most [`MAX_SEARCH_TERMS`] distinct topics.
pub fn topics(query: &str) -> Vec<Topic> {
    let mut terms: Vec<String> = Vec::new();
    for term in normalize(query)
        .split(|character: char| {
            !character.is_alphanumeric() && character != '-' && character != '_'
        })
        .map(|term| term.trim_matches(['-', '_']))
        .filter(|term| term.chars().count() >= 2)
    {
        if !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_owned());
        }
        if terms.len() == MAX_SEARCH_TERMS {
            break;
        }
    }
    terms.iter().map(|term| Topic::new(term)).collect()
}

/// Ranking key: covered topics, topics in the heading, then occurrences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Score {
    pub matched: usize,
    pub heading: usize,
    pub occurrences: usize,
}

/// Which query topics matched a result, as bit masks by topic index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopicMask {
    pub text: u16,
    pub heading: u16,
}

impl Score {
    /// Compact numeric form exposed to clients; preserves the ranking order.
    pub fn value(self) -> usize {
        self.matched * 10_000 + self.heading * 100 + self.occurrences.min(99)
    }
}

pub struct Hit {
    pub score: Score,
    pub topics: TopicMask,
    pub snippet: String,
}

/// Scores `text`, whose title is `heading`, and returns a snippet around the
/// first match. Returns `None` when no topic matches the text.
pub fn score(heading: &str, text: &str, topics: &[Topic]) -> Option<Hit> {
    let normalized: String = normalize_chars(text).collect();
    let normalized_heading = normalize(heading);
    let mut score = Score {
        matched: 0,
        heading: 0,
        occurrences: 0,
    };
    let mut mask = TopicMask::default();
    let mut first_match: Option<usize> = None;
    for (index, topic) in topics.iter().enumerate() {
        let matches = topic.matches(&normalized);
        let Some(first) = matches.first() else {
            continue;
        };
        score.matched += 1;
        score.occurrences += matches.len().min(MAX_OCCURRENCES_PER_TOPIC);
        mask.text |= 1 << index;
        if !topic.matches(&normalized_heading).is_empty() {
            score.heading += 1;
            mask.heading |= 1 << index;
        }
        first_match = Some(first_match.map_or(*first, |current| current.min(*first)));
    }
    let first = first_match?;
    Some(Hit {
        score,
        topics: mask,
        snippet: snippet(text, first),
    })
}

/// Selects up to `limit` hit indexes from `hits`, already sorted best first.
///
/// Coverage ranking alone lets a multi-topic query drown a topic that no
/// result shares with the others. For each topic, the best result whose
/// heading is about that topic is therefore guaranteed a slot; the remaining
/// slots follow the ranking. The selection is returned in ranking order.
pub fn select(hits: &[(Score, TopicMask)], topic_count: usize, limit: usize) -> Vec<usize> {
    let mut selected: Vec<usize> = Vec::new();
    for topic in 0..topic_count {
        let bit = 1 << topic;
        if let Some(index) = hits.iter().position(|(_, mask)| mask.heading & bit != 0)
            && !selected.contains(&index)
        {
            selected.push(index);
        }
    }
    selected.sort_unstable();
    selected.truncate(limit);
    for index in 0..hits.len() {
        if selected.len() >= limit {
            break;
        }
        if !selected.contains(&index) {
            selected.push(index);
        }
    }
    selected.sort_unstable();
    selected
}

fn snippet(text: &str, first: usize) -> String {
    let original: Vec<char> = text.chars().collect();
    let start = first.saturating_sub(SNIPPET_CHARS / 3);
    let end = (start + SNIPPET_CHARS).min(original.len());
    let mut snippet = original[start..end]
        .iter()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if start > 0 {
        snippet.insert(0, '…');
    }
    if end < original.len() {
        snippet.push('…');
    }
    snippet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(query: &str) -> Vec<String> {
        topics(query).into_iter().map(|topic| topic.term).collect()
    }

    #[test]
    fn topics_are_normalized_deduplicated_and_bounded() {
        assert_eq!(
            terms("Idempotência, IDEMPOTENCIA x Idempotency-Key -"),
            ["idempotencia", "idempotency-key"]
        );
        let many = (0..20).map(|index| format!("t{index}")).collect::<Vec<_>>();
        assert_eq!(terms(&many.join(" ")).len(), MAX_SEARCH_TERMS);
    }

    #[test]
    fn synonyms_bridge_english_queries_and_portuguese_guides() {
        let topics = topics("idempotency agent scope");
        let hit = score(
            "Idempotencia",
            "## Idempotencia\nEscopo do agente.",
            &topics,
        )
        .expect("match");
        assert_eq!(hit.score.matched, 3);
        assert_eq!(hit.score.heading, 1);
        assert!(score("Outro", "Sem relacao.", &topics).is_none());
    }

    #[test]
    fn composite_terms_match_whole_or_all_parts() {
        let topics = topics("idempotency-key");
        assert!(score("", "Envie Idempotency-Key.", &topics).is_some());
        assert!(score("", "A chave (key) de idempotencia.", &topics).is_some());
        // A single generic part is not enough.
        assert!(score("", "Use a key do cliente.", &topics).is_none());
    }

    #[test]
    fn ranks_topic_coverage_before_occurrences() {
        let topics = topics("slug cursor");
        let covering = score("", "slug e cursor", &topics).expect("covering").score;
        let repeated = score("", "slug slug slug slug slug", &topics)
            .expect("repeated")
            .score;
        assert!(covering > repeated);
        assert!(covering.value() > repeated.value());
    }

    #[test]
    fn selection_guarantees_one_heading_result_per_topic() {
        let score = |matched| Score {
            matched,
            heading: 0,
            occurrences: 0,
        };
        let mask = |text, heading| TopicMask { text, heading };
        // Topic 0 is only the heading of the last hit; topics 1 and 2 dominate.
        let hits = [
            (score(2), mask(0b110, 0)),
            (score(2), mask(0b110, 0)),
            (score(2), mask(0b110, 0b100)),
            (score(1), mask(0b001, 0b001)),
        ];
        assert_eq!(select(&hits, 3, 2), [2, 3]);
        assert_eq!(select(&hits, 3, 3), [0, 2, 3]);
        assert_eq!(select(&hits, 3, 10), [0, 1, 2, 3]);
        assert_eq!(select(&hits, 3, 1), [2]);
    }

    #[test]
    fn snippets_are_char_safe_around_accented_text() {
        let text = format!(
            "{}Depois vem a idempotência da operação.",
            "Ação ".repeat(80)
        );
        let hit = score("", &text, &topics("idempotencia")).expect("match");
        assert!(hit.snippet.starts_with('…'));
        assert!(hit.snippet.contains("idempotência"));
    }
}
