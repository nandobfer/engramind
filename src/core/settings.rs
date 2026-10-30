//! Configurações funcionais gerenciadas pela API (desenho §12, D-013). O catálogo `SETTINGS`
//! define chave, tipo, padrão, validação e sobreposição por canal; é a base do teste de
//! contrato 6.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};

use super::{
    Core,
    idempotency::Mutation,
    model::{SettingChange, SettingRow},
    principal::Principal,
};
use crate::{error::ApiError, store::Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingType {
    Integer { min: i64, max: i64 },
}

impl SettingType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Integer { .. } => "integer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSpec {
    pub key: &'static str,
    pub kind: SettingType,
    /// Padrão em JSON.
    pub default: &'static str,
    pub channel_override: bool,
    pub description: &'static str,
    /// Versão em que a chave entrou.
    pub since: &'static str,
}

pub const RATE_PER_MINUTE: &str = "limits.rate_per_minute";
pub const MAX_BATCH: &str = "limits.max_batch";

pub const SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        key: RATE_PER_MINUTE,
        kind: SettingType::Integer {
            min: 1,
            max: 100_000,
        },
        default: "600",
        channel_override: false,
        description: "Requisições por minuto aceitas de cada token; acima disso, 429 RATE_LIMITED.",
        since: "0.2.0",
    },
    SettingSpec {
        key: MAX_BATCH,
        kind: SettingType::Integer { min: 1, max: 1000 },
        default: "100",
        channel_override: false,
        description: "Itens aceitos por operação em lote.",
        since: "0.2.0",
    },
];

pub fn spec(key: &str) -> Option<&'static SettingSpec> {
    SETTINGS.iter().find(|spec| spec.key == key)
}

fn default_value(spec: &SettingSpec) -> Value {
    serde_json::from_str(spec.default).expect("padrao do catalogo e JSON valido")
}

fn unknown(key: &str) -> ApiError {
    ApiError::new(
        "SETTING_UNKNOWN",
        format!("Configuracao desconhecida: {key}"),
    )
    .with_details(json!({
        "key": key,
        "valid": SETTINGS.iter().map(|spec| spec.key).collect::<Vec<_>>(),
    }))
}

/// Confere o valor contra o tipo e o intervalo da chave.
pub fn validate(spec: &SettingSpec, value: &Value) -> Result<(), ApiError> {
    match spec.kind {
        SettingType::Integer { min, max } => match value.as_i64() {
            Some(number) if (min..=max).contains(&number) => Ok(()),
            _ => Err(ApiError::new(
                "SETTING_INVALID",
                format!("{} deve ser um inteiro de {min} a {max}", spec.key),
            )
            .with_details(json!({ "key": spec.key, "type": "integer", "min": min, "max": max }))),
        },
    }
}

/// Valores efetivos em memória (padrões sobrepostos pelos persistidos).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsSnapshot {
    values: BTreeMap<&'static str, Value>,
}

impl Default for SettingsSnapshot {
    fn default() -> Self {
        Self::from_rows(&[])
    }
}

impl SettingsSnapshot {
    pub fn from_rows(rows: &[SettingRow]) -> Self {
        let mut values: BTreeMap<&'static str, Value> = SETTINGS
            .iter()
            .map(|spec| (spec.key, default_value(spec)))
            .collect();
        for row in rows {
            if let Some(spec) = spec(&row.key)
                && let Ok(value) = serde_json::from_str::<Value>(&row.value_json)
                && validate(spec, &value).is_ok()
            {
                values.insert(spec.key, value);
            }
        }
        Self { values }
    }

    fn integer(&self, key: &str) -> i64 {
        self.values
            .get(key)
            .and_then(Value::as_i64)
            .or_else(|| spec(key).and_then(|spec| default_value(spec).as_i64()))
            .unwrap_or_default()
    }

    pub fn rate_per_minute(&self) -> u32 {
        u32::try_from(self.integer(RATE_PER_MINUTE)).unwrap_or(u32::MAX)
    }

    pub fn max_batch(&self) -> u32 {
        u32::try_from(self.integer(MAX_BATCH)).unwrap_or(u32::MAX)
    }
}

/// Uma configuração com o valor efetivo, o padrão e a regra.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SettingView {
    #[schema(example = "limits.rate_per_minute")]
    pub key: String,
    #[schema(value_type = Object, example = 600)]
    pub value: Value,
    #[schema(value_type = Object, example = 600)]
    pub default: Value,
    #[serde(rename = "type")]
    #[schema(example = "integer")]
    pub kind: String,
    pub min: Option<i64>,
    pub max: Option<i64>,
    /// Se aceita sobreposição por canal (a partir da 003).
    pub channel_override: bool,
    pub description: String,
    /// `null` quando vale o padrão.
    pub updated_at: Option<String>,
    pub updated_by_app: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SettingsView {
    pub settings: Vec<SettingView>,
}

#[derive(Debug, Clone, Default, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct GetSettingsQuery {
    /// Uma chave só; omita para todas.
    #[param(example = "limits.rate_per_minute")]
    pub key: Option<String>,
}

/// Mapa parcial `chave → valor`; `null` volta ao padrão.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[schema(example = json!({ "limits.rate_per_minute": 1200 }))]
pub struct UpdateSettingsBody(pub BTreeMap<String, Value>);

pub fn view(rows: &[SettingRow], key: Option<&str>) -> Result<SettingsView, ApiError> {
    if let Some(key) = key
        && spec(key).is_none()
    {
        return Err(unknown(key));
    }
    let settings = SETTINGS
        .iter()
        .filter(|spec| key.is_none_or(|key| key == spec.key))
        .map(|spec| {
            let row = rows.iter().find(|row| row.key == spec.key);
            let default = default_value(spec);
            let value = row
                .and_then(|row| serde_json::from_str(&row.value_json).ok())
                .unwrap_or_else(|| default.clone());
            let (min, max) = match spec.kind {
                SettingType::Integer { min, max } => (Some(min), Some(max)),
            };
            SettingView {
                key: spec.key.to_owned(),
                value,
                default,
                kind: spec.kind.name().to_owned(),
                min,
                max,
                channel_override: spec.channel_override,
                description: spec.description.to_owned(),
                updated_at: row.map(|row| row.updated_at.clone()),
                updated_by_app: row.and_then(|row| row.updated_by_app.clone()),
            }
        })
        .collect();
    Ok(SettingsView { settings })
}

impl<S: Store> Core<S> {
    pub async fn get_settings(
        &self,
        principal: &Principal,
        query: GetSettingsQuery,
    ) -> Result<SettingsView, ApiError> {
        self.authorize(principal, "get_settings")?;
        let rows = self.store().get_settings().await?;
        view(&rows, query.key.as_deref().map(str::trim))
    }

    pub async fn update_settings(
        &self,
        principal: &Principal,
        body: UpdateSettingsBody,
        key: Option<&str>,
    ) -> Result<Mutation<SettingsView>, ApiError> {
        let operation = self.authorize(principal, "update_settings")?;
        if body.0.is_empty() {
            return Err(ApiError::validation("Informe ao menos uma configuracao"));
        }
        let mut changes = Vec::with_capacity(body.0.len());
        for (name, value) in &body.0 {
            let spec = spec(name).ok_or_else(|| unknown(name))?;
            let value_json = if value.is_null() {
                None
            } else {
                validate(spec, value)?;
                Some(value.to_string())
            };
            changes.push(SettingChange {
                key: spec.key.to_owned(),
                value_json,
            });
        }
        let core = self.clone();
        let mutation = self
            .mutate(
                principal,
                operation,
                key,
                &body,
                200,
                |context| async move {
                    core.store()
                        .update_settings(changes, context)
                        .await
                        .map_err(|error| super::store_error(error, ApiError::internal))
                },
            )
            .await?;
        self.replace_settings(SettingsSnapshot::from_rows(&mutation.value));
        Ok(Mutation {
            value: view(&mutation.value, None)?,
            replayed: mutation.replayed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_validated_against_the_range() {
        let rate = spec(RATE_PER_MINUTE).expect("rate");
        assert!(validate(rate, &json!(1)).is_ok());
        assert!(validate(rate, &json!(100_000)).is_ok());
        for invalid in [
            json!(0),
            json!(100_001),
            json!(1.5),
            json!("600"),
            json!(-1),
        ] {
            assert_eq!(
                validate(rate, &invalid).expect_err("invalid").code,
                "SETTING_INVALID",
                "{invalid}"
            );
        }
    }

    #[test]
    fn snapshot_uses_defaults_and_ignores_invalid_rows() {
        let snapshot = SettingsSnapshot::default();
        assert_eq!(snapshot.rate_per_minute(), 600);
        assert_eq!(snapshot.max_batch(), 100);
        let row = |key: &str, value: &str| SettingRow {
            key: key.to_owned(),
            value_json: value.to_owned(),
            updated_by_app: None,
            updated_at: "2026-10-30T12:00:00.000Z".to_owned(),
        };
        let snapshot =
            SettingsSnapshot::from_rows(&[row(RATE_PER_MINUTE, "5"), row(MAX_BATCH, "\"x\"")]);
        assert_eq!(snapshot.rate_per_minute(), 5);
        assert_eq!(snapshot.max_batch(), 100);
    }

    #[test]
    fn every_default_is_valid() {
        for spec in SETTINGS {
            validate(spec, &default_value(spec)).expect(spec.key);
        }
    }

    #[test]
    fn view_filters_by_key_and_rejects_unknown_keys() {
        let view_all = view(&[], None).expect("all");
        assert_eq!(view_all.settings.len(), SETTINGS.len());
        let one = view(&[], Some(MAX_BATCH)).expect("one");
        assert_eq!(one.settings.len(), 1);
        assert_eq!(
            view(&[], Some("limits.x")).expect_err("unknown").code,
            "SETTING_UNKNOWN"
        );
    }
}
