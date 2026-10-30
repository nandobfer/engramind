//! Paginação por cursor opaco (D-009): base64url de `(chave de ordem, id)`.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::error::ApiError;

pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 200;

/// Posição depois do último item devolvido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub key: String,
    pub id: String,
}

impl Cursor {
    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(format!("{}\n{}", self.key, self.id))
    }

    pub fn decode(value: &str) -> Result<Self, ApiError> {
        let invalid = || ApiError::validation("cursor invalido");
        let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| invalid())?;
        let text = String::from_utf8(bytes).map_err(|_| invalid())?;
        let (key, id) = text.split_once('\n').ok_or_else(invalid)?;
        Ok(Self {
            key: key.to_owned(),
            id: id.to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    pub after: Option<Cursor>,
    pub limit: u32,
}

impl PageRequest {
    /// Valida `cursor` e `limit` (padrão 50, de 1 a 200).
    pub fn new(cursor: Option<&str>, limit: Option<u32>) -> Result<Self, ApiError> {
        let limit = limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(ApiError::validation(format!(
                "limit deve estar entre 1 e {MAX_LIMIT}"
            )));
        }
        let after = cursor
            .filter(|value| !value.is_empty())
            .map(Cursor::decode)
            .transpose()?;
        Ok(Self { after, limit })
    }
}

/// Uma página de itens e o cursor da próxima, quando houver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<Cursor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_and_rejects_garbage() {
        let cursor = Cursor {
            key: "2026-10-30T12:00:00.000Z".to_owned(),
            id: "app_3f9e1a7c".to_owned(),
        };
        assert_eq!(Cursor::decode(&cursor.encode()).expect("decode"), cursor);
        assert_eq!(
            Cursor::decode("%%%").expect_err("garbage").code,
            "VALIDATION_ERROR"
        );
        assert_eq!(
            Cursor::decode(&URL_SAFE_NO_PAD.encode("sem-separador"))
                .expect_err("format")
                .code,
            "VALIDATION_ERROR"
        );
    }

    #[test]
    fn limit_defaults_to_fifty_and_is_bounded() {
        assert_eq!(PageRequest::new(None, None).expect("default").limit, 50);
        assert!(PageRequest::new(None, Some(200)).is_ok());
        assert!(PageRequest::new(None, Some(0)).is_err());
        assert!(PageRequest::new(None, Some(201)).is_err());
    }
}
