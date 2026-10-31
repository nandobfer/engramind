//! Cofre dos segredos de configuração (desenho §12 e §18, decisão 0015): AES-256-GCM com a
//! chave `SECRETS_KEY`, nonce aleatório a cada gravação e o nome da chave como dado associado.
//! O valor só é gravado; a leitura mostra `{ configured, updated_at }`.

use std::fmt;

use aes_gcm::{
    Aes256Gcm, Key, KeyInit,
    aead::{Aead, AeadCore, OsRng, Payload},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use zeroize::Zeroizing;

use super::{
    Core,
    idempotency::Mutation,
    model::{SecretView, double_option},
    principal::Principal,
    settings::{self, SettingType},
};
use crate::{config::SecretKey, error::ApiError, store::Store};

const FORMAT: &[u8] = b"v1";
const NONCE_LEN: usize = 12;
const MAX_SECRET_CHARS: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub struct VaultError;

/// Cifra e decifra os segredos. A chave fica zerada ao sair de escopo (`SecretKey`).
#[derive(Clone)]
pub struct Vault {
    key: SecretKey,
}

impl Vault {
    pub fn new(key: SecretKey) -> Self {
        Self { key }
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(self.key.expose()))
    }

    /// `v1 || nonce(12) || ciphertext`, com `name` como dado associado.
    pub fn seal(&self, name: &str, plaintext: &str) -> Vec<u8> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher()
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: name.as_bytes(),
                },
            )
            .expect("AES-GCM nao falha ao cifrar");
        let mut sealed = Vec::with_capacity(FORMAT.len() + NONCE_LEN + ciphertext.len());
        sealed.extend_from_slice(FORMAT);
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&ciphertext);
        sealed
    }

    pub fn open(&self, name: &str, sealed: &[u8]) -> Result<Zeroizing<String>, VaultError> {
        let rest = sealed.strip_prefix(FORMAT).ok_or(VaultError)?;
        if rest.len() < NONCE_LEN {
            return Err(VaultError);
        }
        let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
        let plaintext = Zeroizing::new(
            self.cipher()
                .decrypt(
                    nonce.into(),
                    Payload {
                        msg: ciphertext,
                        aad: name.as_bytes(),
                    },
                )
                .map_err(|_| VaultError)?,
        );
        String::from_utf8(plaintext.to_vec())
            .map(Zeroizing::new)
            .map_err(|_| VaultError)
    }
}

/// Corpo de `set_secret`: o valor em claro, que nunca volta em resposta nenhuma.
#[derive(Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSecretBody {
    /// Valor do segredo (até 4096 caracteres); `null` apaga.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>, example = "sk-...")]
    #[schemars(with = "Option<String>")]
    pub value: Option<Option<String>>,
}

impl fmt::Debug for SetSecretBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SetSecretBody([REDACTED])")
    }
}

pub(crate) fn secret_unknown(key: &str) -> ApiError {
    ApiError::new("SECRET_UNKNOWN", format!("Segredo desconhecido: {key}")).with_details(json!({
        "key": key,
        "valid": secret_keys(),
    }))
}

/// Chaves do catálogo marcadas como segredo.
pub fn secret_keys() -> Vec<&'static str> {
    settings::SETTINGS
        .iter()
        .filter(|spec| spec.kind == SettingType::Secret)
        .map(|spec| spec.key)
        .collect()
}

impl<S: Store> Core<S> {
    pub async fn set_secret(
        &self,
        principal: &Principal,
        name: &str,
        body: SetSecretBody,
        key: Option<&str>,
    ) -> Result<Mutation<SecretView>, ApiError> {
        let operation = self.authorize(principal, "set_secret")?;
        let name = name.trim();
        if !secret_keys().contains(&name) {
            return Err(secret_unknown(name));
        }
        let Some(value) = body.value.clone() else {
            return Err(ApiError::validation(
                "Informe value (o segredo) ou null para apagar",
            ));
        };
        let value = value.map(|value| Zeroizing::new(value.trim().to_owned()));
        if let Some(value) = &value {
            let length = value.chars().count();
            if length == 0 || length > MAX_SECRET_CHARS {
                return Err(ApiError::validation(format!(
                    "value deve ter de 1 a {MAX_SECRET_CHARS} caracteres"
                )));
            }
        }
        let sealed = value
            .as_ref()
            .map(|value| self.vault().seal(name, value.as_str()));
        let input = json!({ "key": name, "value": body.value.flatten() });
        let core = self.clone();
        let target = name.to_owned();
        self.mutate(
            principal,
            operation,
            key,
            &input,
            200,
            |context| async move {
                core.store()
                    .set_secret(target, sealed, context)
                    .await
                    .map_err(|error| super::store_error(error, ApiError::internal))
            },
        )
        .await
    }

    /// Valor em claro de um segredo; `None` sem valor gravado ou quando não decifra (outra
    /// `SECRETS_KEY`), com `WARN` sem o valor.
    pub(crate) async fn secret(&self, name: &str) -> Result<Option<Zeroizing<String>>, ApiError> {
        let Some(stored) = self.store().get_secret(name.to_owned()).await? else {
            return Ok(None);
        };
        match self.vault().open(name, &stored.value_enc) {
            Ok(value) => Ok(Some(value)),
            Err(VaultError) => {
                tracing::warn!(
                    event = "secrets.decrypt_failed",
                    key = name,
                    "segredo nao decifra com a SECRETS_KEY atual; tratado como nao configurado"
                );
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(byte: u8) -> Vault {
        Vault::new(SecretKey::new([byte; 32]))
    }

    #[test]
    fn sealed_values_round_trip_with_a_fresh_nonce_each_time() {
        let vault = vault(7);
        let first = vault.seal("redator.api_key", "sk-teste");
        let second = vault.seal("redator.api_key", "sk-teste");
        assert!(first.starts_with(b"v1"));
        assert_ne!(first, second, "nonce unico por gravacao");
        assert_ne!(first[2..14], second[2..14]);
        assert!(
            !first.windows(8).any(|window| window == b"sk-teste"),
            "texto em claro no cifrado"
        );
        assert_eq!(
            vault
                .open("redator.api_key", &first)
                .expect("open")
                .as_str(),
            "sk-teste"
        );
    }

    #[test]
    fn opening_fails_with_another_key_name_tampering_or_format() {
        let vault = vault(7);
        let sealed = vault.seal("redator.api_key", "sk-teste");
        assert_eq!(vault.open("embedding.api_key", &sealed), Err(VaultError));
        assert_eq!(
            self::vault(8).open("redator.api_key", &sealed),
            Err(VaultError)
        );
        let mut tampered = sealed.clone();
        *tampered.last_mut().expect("byte") ^= 1;
        assert_eq!(vault.open("redator.api_key", &tampered), Err(VaultError));
        assert_eq!(vault.open("redator.api_key", &sealed[1..]), Err(VaultError));
        assert_eq!(vault.open("redator.api_key", b"v1"), Err(VaultError));
    }

    #[test]
    fn the_body_never_prints_the_value() {
        let body = SetSecretBody {
            value: Some(Some("sk-teste".to_owned())),
        };
        assert!(!format!("{body:?}").contains("sk-teste"));
    }
}
