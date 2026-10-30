//! Tokens `eng.<token_id>.<segredo>` (D-001, portado do sap-middleman): segredo de 32 bytes
//! aleatórios em base64url sem padding, guardado como HMAC-SHA256 com o `TOKEN_PEPPER` como
//! chave e conferido em tempo constante (`Mac::verify_slice`).

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use sha2::Sha256;
use thiserror::Error;

use crate::{
    config::Secret,
    core::ids::{TOKEN_PREFIX, is_valid},
};

type HmacSha256 = Hmac<Sha256>;

pub const TOKEN_SCHEME: &str = "eng";

/// Segredo recém-gerado: o texto aparece uma única vez; só o digest é persistido.
pub struct GeneratedSecret {
    pub secret: Secret,
    pub digest: Vec<u8>,
}

#[derive(Clone)]
pub struct TokenService {
    pepper: Secret,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("formato de token invalido")]
    InvalidFormat,
    #[error("token invalido")]
    InvalidToken,
}

/// Token decomposto, ainda não verificado.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedToken<'a> {
    pub id: &'a str,
    pub secret: &'a str,
}

impl TokenService {
    pub fn new(pepper: Secret) -> Self {
        Self { pepper }
    }

    pub fn generate(&self) -> GeneratedSecret {
        let mut random = [0_u8; 32];
        OsRng.fill_bytes(&mut random);
        let secret = URL_SAFE_NO_PAD.encode(random);
        let digest = self.digest(&secret);
        GeneratedSecret {
            secret: Secret::new(secret),
            digest,
        }
    }

    /// Texto completo do token, mostrado só na emissão.
    pub fn plaintext(id: &str, secret: &Secret) -> Secret {
        Secret::new(format!("{TOKEN_SCHEME}.{id}.{}", secret.expose()))
    }

    pub fn digest(&self, secret: &str) -> Vec<u8> {
        let mut mac = self.mac();
        mac.update(secret.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }

    /// Confere o segredo contra o digest persistido, em tempo constante.
    pub fn verify(&self, secret: &str, expected_digest: &[u8]) -> Result<(), TokenError> {
        let mut mac = self.mac();
        mac.update(secret.as_bytes());
        mac.verify_slice(expected_digest)
            .map_err(|_| TokenError::InvalidToken)
    }

    fn mac(&self) -> HmacSha256 {
        HmacSha256::new_from_slice(self.pepper.expose().as_bytes())
            .expect("HMAC accepts keys of any length")
    }
}

/// Parser estrito: `eng`, id `tk_` + 8 hex minúsculos e segredo com 32 ou mais caracteres
/// `[A-Za-z0-9_-]`.
pub fn parse(token: &str) -> Result<ParsedToken<'_>, TokenError> {
    let mut parts = token.split('.');
    let scheme = parts.next();
    let id = parts.next();
    let secret = parts.next();
    if scheme != Some(TOKEN_SCHEME) || parts.next().is_some() {
        return Err(TokenError::InvalidFormat);
    }
    let id = id
        .filter(|value| is_valid(TOKEN_PREFIX, value))
        .ok_or(TokenError::InvalidFormat)?;
    let secret = secret
        .filter(|value| {
            value.len() >= 32
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or(TokenError::InvalidFormat)?;
    Ok(ParsedToken { id, secret })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> TokenService {
        TokenService::new(Secret::new(
            "unit-test-pepper-with-more-than-32-characters".to_owned(),
        ))
    }

    #[test]
    fn generated_token_can_be_verified() {
        let service = service();
        let generated = service.generate();
        let plaintext = TokenService::plaintext("tk_3f9e1a7c", &generated.secret);
        let parsed = parse(plaintext.expose()).expect("parse");
        assert_eq!(parsed.id, "tk_3f9e1a7c");
        service
            .verify(parsed.secret, &generated.digest)
            .expect("generated token should verify");
        assert_eq!(generated.secret.expose().len(), 43);
    }

    #[test]
    fn rejects_malformed_or_changed_tokens() {
        let service = service();
        let generated = service.generate();
        for malformed in [
            "invalid",
            "eng.tk_3f9e1a7c",
            "smm.tk_3f9e1a7c.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "eng.3f9e1a7c.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "eng.tk_3F9E1A7C.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "eng.tk_3f9e1a7c.curto",
            "eng.tk_3f9e1a7c.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!",
            "eng.tk_3f9e1a7c.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.extra",
        ] {
            assert_eq!(
                parse(malformed),
                Err(TokenError::InvalidFormat),
                "{malformed}"
            );
        }
        assert_eq!(
            service.verify(&"x".repeat(43), &generated.digest),
            Err(TokenError::InvalidToken)
        );
        let other = TokenService::new(Secret::new("outro-pepper-com-mais-de-32-caracteres".into()));
        assert_eq!(
            other.verify(generated.secret.expose(), &generated.digest),
            Err(TokenError::InvalidToken)
        );
    }
}
