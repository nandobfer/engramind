//! `version` do contexto (005 D-001, Q-008): `cv_` + 12 hex do SHA-256 sobre o formato, os
//! canais alcançados `(id, version, modo, prioridade)` e as configurações que mudam o bloco. O
//! digest completo é a chave do cache (Q-009).

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::ContextFormat;

pub type Fingerprint = [u8; 32];

/// Um canal alcançado como entra no hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachedChannel<'a> {
    pub id: &'a str,
    pub version: i64,
    pub mode: &'a str,
    pub priority: i64,
}

pub fn fingerprint(
    format: ContextFormat,
    channels: &[ReachedChannel<'_>],
    settings: &Value,
) -> Fingerprint {
    let mut channels: Vec<&ReachedChannel<'_>> = channels.iter().collect();
    channels.sort_by(|a, b| a.id.cmp(b.id));
    let mut hasher = Sha256::new();
    hasher.update(b"engramind.context.v1\n");
    hasher.update(format.as_str().as_bytes());
    hasher.update(b"\n");
    for channel in channels {
        hasher.update(
            format!(
                "{}:{}:{}:{}\n",
                channel.id, channel.version, channel.mode, channel.priority
            )
            .as_bytes(),
        );
    }
    hasher.update(settings.to_string().as_bytes());
    hasher.finalize().into()
}

pub fn label(fingerprint: &Fingerprint) -> String {
    let hex: String = fingerprint[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("cv_{hex}")
}

/// `If-None-Match` contra a `version` (comparação fraca da RFC 9110): lista separada por
/// vírgula, com ou sem `W/` e aspas. `*` não vale.
pub fn matches(if_none_match: &str, version: &str) -> bool {
    if_none_match.split(',').any(|candidate| {
        let candidate = candidate.trim();
        let candidate = candidate.strip_prefix("W/").unwrap_or(candidate);
        let candidate = candidate
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .unwrap_or(candidate);
        candidate == version
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn channel(id: &str, version: i64) -> ReachedChannel<'_> {
        ReachedChannel {
            id,
            version,
            mode: "inject",
            priority: 0,
        }
    }

    #[test]
    fn the_version_is_stable_and_order_independent() {
        let settings = json!({ "context.budget_tokens": 4000 });
        let a = fingerprint(
            ContextFormat::Tagged,
            &[channel("ch_1", 1), channel("ch_2", 3)],
            &settings,
        );
        let b = fingerprint(
            ContextFormat::Tagged,
            &[channel("ch_2", 3), channel("ch_1", 1)],
            &settings,
        );
        assert_eq!(a, b);
        assert_eq!(label(&a).len(), 15);
        assert!(label(&a).starts_with("cv_"));
    }

    #[test]
    fn every_input_changes_the_version() {
        let settings = json!({ "context.budget_tokens": 4000 });
        let base = fingerprint(ContextFormat::Tagged, &[channel("ch_1", 1)], &settings);
        let changed = [
            fingerprint(ContextFormat::Markdown, &[channel("ch_1", 1)], &settings),
            fingerprint(ContextFormat::Tagged, &[channel("ch_1", 2)], &settings),
            fingerprint(
                ContextFormat::Tagged,
                &[ReachedChannel {
                    priority: 5,
                    ..channel("ch_1", 1)
                }],
                &settings,
            ),
            fingerprint(
                ContextFormat::Tagged,
                &[channel("ch_1", 1), channel("ch_2", 1)],
                &settings,
            ),
            fingerprint(
                ContextFormat::Tagged,
                &[channel("ch_1", 1)],
                &json!({ "context.budget_tokens": 10 }),
            ),
        ];
        for other in changed {
            assert_ne!(base, other);
        }
    }

    #[test]
    fn if_none_match_accepts_lists_quotes_and_weak_tags() {
        assert!(matches("cv_3f9e1a000000", "cv_3f9e1a000000"));
        assert!(matches("\"cv_3f9e1a000000\"", "cv_3f9e1a000000"));
        assert!(matches(
            "\"cv_000000000000\", W/\"cv_3f9e1a000000\"",
            "cv_3f9e1a000000"
        ));
        assert!(!matches("*", "cv_3f9e1a000000"));
        assert!(!matches("\"cv_000000000000\"", "cv_3f9e1a000000"));
    }
}
