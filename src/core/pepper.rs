//! Verificador do `TOKEN_PEPPER` (D-008): um HMAC de contexto fixo gravado em `schema_meta`
//! impede o startup com um pepper diferente do usado no banco.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;

use crate::{
    config::Secret,
    store::{Store, StoreError},
};

type HmacSha256 = Hmac<Sha256>;

const PEPPER_CHECK_CONTEXT: &[u8] = b"engramind:token-pepper:v1";
pub const PEPPER_CHECK_KEY: &str = "token_pepper_check";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PepperCheck {
    /// Banco sem verificador: o pepper atual foi registrado.
    Enrolled,
    /// O pepper atual corresponde ao registrado.
    Verified,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PepperError {
    #[error("TOKEN_PEPPER nao corresponde ao banco existente")]
    Mismatch,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub async fn enforce<S: Store>(store: &S, pepper: &Secret) -> Result<PepperCheck, PepperError> {
    match store.meta(PEPPER_CHECK_KEY).await? {
        Some(expected) => {
            mac(pepper)
                .verify_slice(&expected)
                .map_err(|_| PepperError::Mismatch)?;
            Ok(PepperCheck::Verified)
        }
        None => {
            let check = mac(pepper).finalize().into_bytes();
            store.set_meta(PEPPER_CHECK_KEY, &check).await?;
            Ok(PepperCheck::Enrolled)
        }
    }
}

fn mac(pepper: &Secret) -> HmacSha256 {
    let mut mac = HmacSha256::new_from_slice(pepper.expose().as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(PEPPER_CHECK_CONTEXT);
    mac
}
