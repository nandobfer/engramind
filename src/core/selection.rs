//! Slugs de canal, padrões de alvo (exato ou prefixo terminado em `*`) e a seleção por
//! requisição (`channels` e `Engramind-Channels`; desenho §5, D-003).

use crate::error::ApiError;

pub const MAX_SLUG_CHARS: usize = 120;
pub const MAX_SEGMENTS: usize = 8;
/// Padrões aceitos numa lista de seleção.
pub const MAX_PATTERNS: usize = 64;

fn segment_chars(segment: &str) -> bool {
    segment
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !segment.starts_with('-')
        && !segment.contains("--")
}

/// Segmento completo: `[a-z0-9]` no início e no fim, hífens simples no meio.
fn valid_segment(segment: &str) -> bool {
    !segment.is_empty() && segment_chars(segment) && !segment.ends_with('-')
}

/// Início de segmento num prefixo parcial (`projeto-` em `eng/projeto-*`); pode ser vazio.
fn valid_partial(segment: &str) -> bool {
    segment.is_empty() || segment_chars(segment)
}

fn slug_error() -> ApiError {
    ApiError::validation(format!(
        "slug de canal deve ter ate {MAX_SEGMENTS} segmentos e {MAX_SLUG_CHARS} caracteres, \
         separados por /; cada segmento com minusculas, digitos e hifens simples"
    ))
}

/// Confere o slug de um canal: `eng/global`, `eng/projeto-atlas` (FR-001).
pub fn validate_channel_slug(slug: &str) -> Result<(), ApiError> {
    let segments: Vec<&str> = slug.split('/').collect();
    let valid = (1..=MAX_SLUG_CHARS).contains(&slug.len())
        && segments.len() <= MAX_SEGMENTS
        && segments.iter().all(|segment| valid_segment(segment));
    if valid { Ok(()) } else { Err(slug_error()) }
}

/// Alvo de uma assinatura ou item de uma seleção.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelPattern {
    /// Um canal exato.
    Exact(String),
    /// Prefixo sem o `*` final: `eng/` para `eng/*`, `eng/projeto-` para `eng/projeto-*`.
    Prefix(String),
}

impl ChannelPattern {
    /// Lê `eng/global`, `eng/*` ou `eng/projeto-*`. O prefixo precisa de ao menos um segmento
    /// completo e da `/` antes do `*` (Q-004, Q-005).
    pub fn parse(value: &str) -> Result<Self, ApiError> {
        let value = value.trim();
        let Some(stem) = value.strip_suffix('*') else {
            validate_channel_slug(value)?;
            return Ok(Self::Exact(value.to_owned()));
        };
        let invalid = || {
            ApiError::validation(format!(
                "prefixo invalido: {value}; use ao menos um segmento completo e / antes do * \
                 (eng/* ou eng/projeto-*)"
            ))
        };
        let (head, partial) = stem.rsplit_once('/').ok_or_else(invalid)?;
        let segments = head.split('/').count() + 1;
        if validate_channel_slug(head).is_err()
            || !valid_partial(partial)
            || segments > MAX_SEGMENTS
            || stem.len() >= MAX_SLUG_CHARS
        {
            return Err(invalid());
        }
        Ok(Self::Prefix(stem.to_owned()))
    }

    /// Reconstrói o padrão a partir da forma persistida (`target`, `is_prefix`).
    pub fn from_stored(target: &str, is_prefix: bool) -> Self {
        if is_prefix {
            Self::Prefix(target.to_owned())
        } else {
            Self::Exact(target.to_owned())
        }
    }

    /// Forma persistida: `(target sem o *, is_prefix)`.
    pub fn stored(&self) -> (&str, bool) {
        match self {
            Self::Exact(slug) => (slug, false),
            Self::Prefix(stem) => (stem, true),
        }
    }

    /// Forma pública: `eng/global` ou `eng/*`.
    pub fn display(&self) -> String {
        match self {
            Self::Exact(slug) => slug.clone(),
            Self::Prefix(stem) => format!("{stem}*"),
        }
    }

    pub fn matches(&self, slug: &str) -> bool {
        match self {
            Self::Exact(exact) => exact == slug,
            Self::Prefix(stem) => slug.starts_with(stem.as_str()),
        }
    }

    /// Ordem de especificidade (FR-005): exato vence prefixo; prefixo mais longo vence o curto.
    pub fn specificity(&self) -> (bool, usize) {
        match self {
            Self::Exact(slug) => (true, slug.len()),
            Self::Prefix(stem) => (false, stem.len()),
        }
    }
}

/// Seleção da requisição: o cabeçalho `Engramind-Channels` e o parâmetro `channels`. Com os
/// dois, vale a interseção (D-003); a seleção só restringe o que o token já alcança.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    lists: Vec<Vec<ChannelPattern>>,
}

fn parse_list(source: &str, value: Option<&str>) -> Result<Option<Vec<ChannelPattern>>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let patterns = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            ChannelPattern::parse(item).map_err(|error| {
                ApiError::validation(format!("{source}: {}", error.message))
                    .with_details(serde_json::json!({ "source": source, "pattern": item }))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if patterns.len() > MAX_PATTERNS {
        return Err(ApiError::validation(format!(
            "{source}: no maximo {MAX_PATTERNS} canais ou prefixos"
        )));
    }
    Ok((!patterns.is_empty()).then_some(patterns))
}

impl Selection {
    /// Sem restrição.
    pub fn all() -> Self {
        Self::default()
    }

    /// Lê o cabeçalho e o parâmetro (listas separadas por vírgula; vazio = ausente).
    pub fn parse(header: Option<&str>, channels: Option<&str>) -> Result<Self, ApiError> {
        let lists = [
            parse_list("Engramind-Channels", header)?,
            parse_list("channels", channels)?,
        ]
        .into_iter()
        .flatten()
        .collect();
        Ok(Self { lists })
    }

    pub fn is_restricted(&self) -> bool {
        !self.lists.is_empty()
    }

    /// O canal passa por todas as listas presentes.
    pub fn admits(&self, slug: &str) -> bool {
        self.lists
            .iter()
            .all(|list| list.iter().any(|pattern| pattern.matches(slug)))
    }

    /// O canal é citado pelo slug exato em alguma lista e passa pelas demais (Q-007).
    pub fn cites(&self, slug: &str) -> bool {
        self.admits(slug)
            && self.lists.iter().any(|list| {
                list.iter()
                    .any(|pattern| matches!(pattern, ChannelPattern::Exact(exact) if exact == slug))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_slugs_are_hierarchical_and_bounded() {
        for valid in [
            "eng",
            "eng/global",
            "eng/projeto-atlas",
            "a/b/c/d/e/f/g/h",
            "x1/2y",
        ] {
            assert!(validate_channel_slug(valid).is_ok(), "{valid}");
        }
        let long = format!("a/{}", "b".repeat(MAX_SLUG_CHARS - 1));
        for invalid in [
            "",
            "Eng",
            "eng/",
            "/eng",
            "eng//x",
            "eng/-x",
            "eng/x-",
            "eng/a--b",
            "eng/x y",
            "eng/*",
            "a/b/c/d/e/f/g/h/i",
            long.as_str(),
        ] {
            assert!(validate_channel_slug(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn prefixes_need_a_complete_segment_before_the_star() {
        assert_eq!(
            ChannelPattern::parse("eng/*").expect("eng/*"),
            ChannelPattern::Prefix("eng/".to_owned())
        );
        assert_eq!(
            ChannelPattern::parse("eng/projeto-*").expect("partial"),
            ChannelPattern::Prefix("eng/projeto-".to_owned())
        );
        assert_eq!(
            ChannelPattern::parse(" eng/global ").expect("exact"),
            ChannelPattern::Exact("eng/global".to_owned())
        );
        for invalid in [
            "*",
            "eng*",
            "/*",
            "eng/-*",
            "eng/a--*",
            "Eng/*",
            "eng/**",
            "a/b/c/d/e/f/g/h/*",
        ] {
            assert!(ChannelPattern::parse(invalid).is_err(), "{invalid}");
        }
        let pattern = ChannelPattern::parse("eng/projeto-*").expect("partial");
        assert_eq!(pattern.display(), "eng/projeto-*");
        assert_eq!(pattern.stored(), ("eng/projeto-", true));
        assert_eq!(ChannelPattern::from_stored("eng/projeto-", true), pattern);
    }

    #[test]
    fn prefix_reaches_descendants_but_not_the_parent() {
        let prefix = ChannelPattern::parse("eng/*").expect("prefix");
        assert!(prefix.matches("eng/x"));
        assert!(prefix.matches("eng/x/y"));
        assert!(!prefix.matches("eng"));
        assert!(!prefix.matches("engenharia/x"));
        let partial = ChannelPattern::parse("eng/projeto-*").expect("partial");
        assert!(partial.matches("eng/projeto-atlas"));
        assert!(partial.matches("eng/projeto-atlas/docs"));
        assert!(!partial.matches("eng/projetos"));
        assert!(!partial.matches("eng/global"));
    }

    #[test]
    fn exact_beats_prefix_and_longer_prefix_beats_shorter() {
        let exact = ChannelPattern::parse("eng/projeto-atlas").expect("exact");
        let long = ChannelPattern::parse("eng/projeto-*").expect("long");
        let short = ChannelPattern::parse("eng/*").expect("short");
        assert!(exact.specificity() > long.specificity());
        assert!(long.specificity() > short.specificity());
        assert!(ChannelPattern::parse("a/*").expect("a").specificity() < exact.specificity());
    }

    #[test]
    fn header_and_parameter_intersect() {
        let none = Selection::parse(None, Some(" , ")).expect("empty");
        assert!(!none.is_restricted());
        assert!(none.admits("qualquer/canal"));

        let header = Selection::parse(Some("eng/*"), None).expect("header");
        assert!(header.admits("eng/global"));
        assert!(!header.admits("review/revisor"));
        assert!(!header.cites("eng/global"));

        let both =
            Selection::parse(Some("eng/*"), Some("eng/global,review/revisor")).expect("both");
        assert!(both.admits("eng/global"));
        assert!(
            !both.admits("review/revisor"),
            "o parametro nao escapa do cabecalho"
        );
        assert!(!both.admits("eng/projeto-atlas"));
        assert!(both.cites("eng/global"));
        assert!(!both.cites("review/revisor"));
    }

    #[test]
    fn invalid_patterns_name_their_source() {
        let error = Selection::parse(Some("eng/*"), Some("eng*")).expect_err("invalid");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert_eq!(error.details.expect("details")["source"], "channels");
        let many = vec!["eng/x"; MAX_PATTERNS + 1].join(",");
        assert!(Selection::parse(Some(&many), None).is_err());
    }
}
