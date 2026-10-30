//! Funções puras sobre o texto das memórias (desenho §7, D-001): normalização, `content_hash`,
//! título provisório, termos de busca e similaridade de Jaccard.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Comprimento máximo do título provisório, em caracteres.
pub const TITLE_MAX: usize = 100;
/// Termos considerados numa consulta de busca.
pub const MAX_TERMS: usize = 32;

/// NFC, sem espaços nas pontas e com os espaços internos colapsados num só.
pub fn tidy(text: &str) -> String {
    let nfc: String = text.nfc().collect();
    nfc.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// SHA-256 do texto em NFC, sem espaços nas pontas, com espaços colapsados e minúsculo.
pub fn content_hash(text: &str) -> Vec<u8> {
    Sha256::digest(tidy(text).to_lowercase().as_bytes()).to_vec()
}

/// `content_hash` em hexadecimal (auditoria).
pub fn hash_hex(hash: &[u8]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A primeira frase do texto, com até `TITLE_MAX` caracteres; cortada numa palavra e terminada
/// em `…` quando passa do limite.
pub fn provisional_title(text: &str) -> String {
    let first_line = text.trim().lines().next().unwrap_or_default();
    let chars: Vec<char> = first_line.chars().collect();
    let mut end = chars.len();
    for (index, window) in chars.windows(2).enumerate() {
        if matches!(window[0], '.' | '!' | '?') && window[1].is_whitespace() {
            end = index + 1;
            break;
        }
    }
    let sentence: String = chars[..end].iter().collect();
    let sentence = tidy(&sentence);
    let sentence = sentence.trim_end_matches(['.', ' ']).to_owned();
    if sentence.chars().count() <= TITLE_MAX {
        return sentence;
    }
    let limit: String = sentence.chars().take(TITLE_MAX - 1).collect();
    let cut = match limit.rfind(' ') {
        Some(space) if limit[..space].chars().count() >= TITLE_MAX / 2 => &limit[..space],
        _ => limit.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', ' ']))
}

/// Minúsculo e sem acentos.
pub fn fold(text: &str) -> String {
    text.nfd()
        .filter(|character| !is_combining_mark(*character))
        .collect::<String>()
        .to_lowercase()
}

/// Palavras (letras e dígitos) do texto, em ordem e sem repetição, sem acentos e minúsculas.
pub fn terms(text: &str) -> Vec<String> {
    let folded = fold(text);
    let mut seen = BTreeSet::new();
    folded
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .filter(|word| seen.insert((*word).to_owned()))
        .map(str::to_owned)
        .collect()
}

/// Até `MAX_TERMS` termos de uma consulta.
pub fn query_terms(query: &str) -> Vec<String> {
    let mut terms = terms(query);
    terms.truncate(MAX_TERMS);
    terms
}

/// |A ∩ B| / |A ∪ B| sobre os termos dos dois textos; 0 quando ambos são vazios.
pub fn jaccard(a: &str, b: &str) -> f64 {
    let a: BTreeSet<String> = terms(a).into_iter().collect();
    let b: BTreeSet<String> = terms(b).into_iter().collect();
    let union = a.union(&b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(&b).count() as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ignores_case_spacing_and_unicode_form() {
        let composed = "Configuração  do\n serviço ";
        let decomposed = "configuração do servic\u{0327}o";
        assert_eq!(content_hash(composed), content_hash(decomposed));
        assert_ne!(
            content_hash(composed),
            content_hash("configuracao do servico")
        );
        assert_eq!(content_hash("a").len(), 32);
        assert_eq!(hash_hex(&[0, 15, 255]), "000fff");
    }

    #[test]
    fn provisional_title_is_the_first_sentence() {
        assert_eq!(
            provisional_title("Use nextest localmente. Rode a suíte completa só no CI."),
            "Use nextest localmente"
        );
        assert_eq!(provisional_title("  Sem ponto final  "), "Sem ponto final");
        assert_eq!(
            provisional_title("Versão 1.2 do app! Depois"),
            "Versão 1.2 do app!"
        );
        assert_eq!(
            provisional_title("Primeira linha\nsegunda"),
            "Primeira linha"
        );
    }

    #[test]
    fn long_titles_are_cut_at_a_word() {
        let text = "palavra ".repeat(40);
        let title = provisional_title(&text);
        assert!(title.chars().count() <= TITLE_MAX, "{title}");
        assert!(title.ends_with("palavra…"), "{title}");
        let single = "x".repeat(300);
        let title = provisional_title(&single);
        assert_eq!(title.chars().count(), TITLE_MAX);
    }

    #[test]
    fn terms_fold_accents_and_case() {
        assert_eq!(
            terms("Configuração do CI; configuracao, ci-local"),
            vec!["configuracao", "do", "ci", "local"]
        );
        assert_eq!(query_terms(&"a b ".repeat(100)).len(), 2);
        let many: String = (0..100).map(|n| format!("t{n} ")).collect();
        assert_eq!(query_terms(&many).len(), MAX_TERMS);
    }

    #[test]
    fn jaccard_compares_term_sets() {
        assert_eq!(jaccard("a b c", "c b a"), 1.0);
        assert_eq!(jaccard("a b", "c d"), 0.0);
        assert!((jaccard("a b c d", "a b c e") - 0.6).abs() < 1e-9);
        assert_eq!(jaccard("", ""), 0.0);
        assert_eq!(jaccard("Serviço", "servico"), 1.0);
    }
}
