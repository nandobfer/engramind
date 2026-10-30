//! Ids públicos curtos com prefixo de tipo (`app_`, `ag_`, `tk_`, `ch_`, `sub_` + 8 hex minúsculos) e
//! carimbos de tempo RFC 3339 com milissegundos (ordenáveis como texto).

use chrono::{DateTime, SecondsFormat, Utc};
use rand::{RngCore, rngs::OsRng};

pub const APP_PREFIX: &str = "app_";
pub const AGENT_PREFIX: &str = "ag_";
pub const TOKEN_PREFIX: &str = "tk_";
pub const CHANNEL_PREFIX: &str = "ch_";
pub const SUBSCRIPTION_PREFIX: &str = "sub_";

const HEX_LEN: usize = 8;

/// Gera `prefixo` + 8 hex minúsculos. Colisões são tratadas por quem insere (retentativa).
pub fn generate(prefix: &str) -> String {
    let value = OsRng.next_u32();
    format!("{prefix}{value:08x}")
}

/// Confere o formato `prefixo` + 8 hex minúsculos.
pub fn is_valid(prefix: &str, value: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == HEX_LEN
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// Forma persistida e exposta dos carimbos: `2026-10-30T12:00:00.000Z`.
pub fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_have_the_prefix_and_eight_hex_digits() {
        for prefix in [APP_PREFIX, AGENT_PREFIX, TOKEN_PREFIX] {
            let id = generate(prefix);
            assert!(is_valid(prefix, &id), "{id}");
        }
        assert!(!is_valid(TOKEN_PREFIX, "tk_3F9E1A7C"));
        assert!(!is_valid(TOKEN_PREFIX, "tk_3f9e1a7"));
        assert!(!is_valid(TOKEN_PREFIX, "app_3f9e1a7c"));
    }

    #[test]
    fn timestamps_sort_as_text() {
        let earlier = parse_timestamp("2026-10-30T09:00:00Z").expect("t1");
        let later = parse_timestamp("2026-10-30T10:00:00.5Z").expect("t2");
        assert!(timestamp(earlier) < timestamp(later));
        assert_eq!(timestamp(earlier), "2026-10-30T09:00:00.000Z");
    }
}
