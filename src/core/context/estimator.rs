//! Estimativa de tokens (`context.token_estimator`) com `tiktoken-rs`. As tabelas BPE são
//! carregadas só no primeiro uso (decisão 0014); `serve` as aquece depois de subir.

/// Conta os tokens de um texto.
pub trait TokenEstimator: Send + Sync {
    fn count(&self, text: &str) -> u32;
}

/// Codificações aceitas por `context.token_estimator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tiktoken {
    O200k,
    Cl100k,
}

impl Tiktoken {
    /// Valor da configuração; desconhecido cai no padrão `o200k`.
    pub fn from_setting(name: &str) -> Self {
        match name {
            "cl100k" => Self::Cl100k,
            _ => Self::O200k,
        }
    }

    /// Carrega a tabela da codificação.
    pub fn warm(self) {
        self.count("");
    }
}

impl TokenEstimator for Tiktoken {
    fn count(&self, text: &str) -> u32 {
        let bpe = match self {
            Self::O200k => tiktoken_rs::o200k_base_singleton(),
            Self::Cl100k => tiktoken_rs::cl100k_base_singleton(),
        };
        u32::try_from(bpe.encode_ordinary(text).len()).unwrap_or(u32::MAX)
    }
}

/// Estimador determinístico dos unitários: uma palavra por token.
#[cfg(test)]
pub struct Words;

#[cfg(test)]
impl TokenEstimator for Words {
    fn count(&self, text: &str) -> u32 {
        text.split_whitespace().count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_encodings_count_tokens() {
        assert_eq!(Tiktoken::O200k.count(""), 0);
        assert!(Tiktoken::O200k.count("Nunca rodar a suíte completa localmente.") > 3);
        assert!(Tiktoken::Cl100k.count("Nunca rodar a suíte completa localmente.") > 3);
        assert_eq!(Tiktoken::from_setting("cl100k"), Tiktoken::Cl100k);
        assert_eq!(Tiktoken::from_setting("o200k"), Tiktoken::O200k);
    }
}
