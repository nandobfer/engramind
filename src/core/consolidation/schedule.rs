//! Agendador da consolidação (009 FR-008, Q-010): `consolidation.schedule` é uma expressão cron
//! de 5 campos avaliada em UTC com a crate `croner` (decisão 0025).

use chrono::{DateTime, Utc};
use croner::{
    Cron,
    parser::{CronParser, Seconds, Year},
};

/// Chave de `schema_meta` com o último horário disparado.
pub const META_LAST_SCHEDULED: &str = "consolidation.last_scheduled";

/// Valida a expressão: exatamente 5 campos (minuto, hora, dia, mês, dia da semana).
pub fn parse(expression: &str) -> Result<Cron, String> {
    if expression.split_whitespace().count() != 5 {
        return Err("a expressao precisa de 5 campos".to_owned());
    }
    CronParser::builder()
        .seconds(Seconds::Disallowed)
        .year(Year::Disallowed)
        .build()
        .parse(expression)
        .map_err(|error| error.to_string())
}

/// Próximo horário estritamente depois de `after`.
pub fn next_after(expression: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    parse(expression)
        .ok()?
        .find_next_occurrence(&after, false)
        .ok()
}

/// Algum horário em `(last, now]`: dispara uma vez só, mesmo depois de vários perdidos.
pub fn due(expression: &str, last: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    next_after(expression, last).is_some_and(|next| next <= now)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .single()
            .expect("instante valido")
    }

    #[test]
    fn expressions_have_five_fields() {
        assert!(parse("0 3 * * *").is_ok());
        assert!(parse("*/15 * * * 1-5").is_ok());
        assert!(parse("0 0 3 * * *").is_err());
        assert!(parse("@daily").is_err());
        assert!(parse("61 3 * * *").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn next_run_is_in_utc_and_strictly_after() {
        assert_eq!(next_after("0 3 * * *", at(30, 2, 0)), Some(at(30, 3, 0)));
        assert_eq!(next_after("0 3 * * *", at(30, 3, 0)), Some(at(31, 3, 0)));
    }

    #[test]
    fn missed_runs_fire_once() {
        assert!(!due("0 3 * * *", at(30, 3, 0), at(31, 2, 59)));
        assert!(due("0 3 * * *", at(30, 3, 0), at(31, 3, 0)));
        // Desligado por três dias: um disparo só (o chamador grava `now` como último).
        assert!(due("0 3 * * *", at(27, 3, 0), at(30, 12, 0)));
        assert!(!due("0 3 * * *", at(30, 12, 0), at(30, 12, 1)));
    }
}
