//! Configurações funcionais gerenciadas pela API (desenho §12, D-013). O catálogo `SETTINGS`
//! define chave, tipo, padrão, validação e sobreposição por canal; é a base do teste de
//! contrato 6.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};

use super::{
    Core,
    idempotency::Mutation,
    model::{Channel, SettingChange, SettingRow},
    principal::Principal,
};
use crate::{
    error::ApiError,
    store::{Store, StoreError},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingType {
    Integer {
        min: i64,
        max: i64,
    },
    Number {
        min: f64,
        max: f64,
    },
    Boolean,
    /// Lista `{ key, label, priority }` de 1 a `MAX_CATEGORIES` itens.
    Categories,
}

impl SettingType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Integer { .. } => "integer",
            Self::Number { .. } => "number",
            Self::Boolean => "boolean",
            Self::Categories => "categories",
        }
    }

    /// Intervalo como o guia `configuracoes` o mostra.
    pub fn range(self) -> String {
        match self {
            Self::Integer { min, max } => format!("{min} a {max}"),
            Self::Number { min, max } => format!("{min} a {max}"),
            Self::Boolean => "—".to_owned(),
            Self::Categories => format!("1 a {MAX_CATEGORIES} itens"),
        }
    }

    fn bounds(self) -> (Option<serde_json::Number>, Option<serde_json::Number>) {
        match self {
            Self::Integer { min, max } => (Some(min.into()), Some(max.into())),
            Self::Number { min, max } => (
                serde_json::Number::from_f64(min),
                serde_json::Number::from_f64(max),
            ),
            Self::Boolean => (None, None),
            Self::Categories => (Some(1.into()), Some((MAX_CATEGORIES as u64).into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
pub const CATEGORIES: &str = "categories.list";
pub const TEXT_MIN: &str = "memory.text_min";
pub const TEXT_MAX: &str = "memory.text_max";
pub const DEDUPE_EXACT: &str = "memory.dedupe_exact";
pub const SIMILAR_THRESHOLD: &str = "memory.similar_threshold";

pub const MAX_CATEGORIES: usize = 50;
const MAX_CATEGORY_KEY: usize = 40;
const MAX_CATEGORY_LABEL: usize = 80;

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
    SettingSpec {
        key: CATEGORIES,
        kind: SettingType::Categories,
        default: r#"[{"key":"PROJECT_RULES","label":"Regras do projeto","priority":50},{"key":"ARCHITECTURE","label":"Arquitetura","priority":40},{"key":"CONSTRAINTS","label":"Restrições","priority":30},{"key":"CONFIG_VALUES","label":"Valores de configuração","priority":20},{"key":"NAMING","label":"Nomenclatura","priority":10}]"#,
        channel_override: false,
        description: "Categorias aceitas nas memórias, `{ key, label, priority }`; a primeira é a padrão. Retirar uma categoria em uso dá 409 CATEGORY_IN_USE.",
        since: "0.4.0",
    },
    SettingSpec {
        key: TEXT_MIN,
        kind: SettingType::Integer { min: 1, max: 1000 },
        default: "3",
        channel_override: false,
        description: "Caracteres mínimos do texto de uma memória.",
        since: "0.4.0",
    },
    SettingSpec {
        key: TEXT_MAX,
        kind: SettingType::Integer {
            min: 10,
            max: 20_000,
        },
        default: "4000",
        channel_override: true,
        description: "Caracteres máximos do texto de uma memória.",
        since: "0.4.0",
    },
    SettingSpec {
        key: DEDUPE_EXACT,
        kind: SettingType::Boolean,
        default: "true",
        channel_override: false,
        description: "Texto igual (`content_hash` normalizado) a outra memória do canal devolve a existente com `duplicate: true`.",
        since: "0.4.0",
    },
    SettingSpec {
        key: SIMILAR_THRESHOLD,
        kind: SettingType::Number { min: 0.0, max: 1.0 },
        default: "0.85",
        channel_override: false,
        description: "Similaridade mínima para uma memória aparecer em `similar` na resposta de escrita (Jaccard dos termos até haver embeddings).",
        since: "0.4.0",
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

/// Chave que aceita sobreposição por canal; as outras → `400 SETTING_UNKNOWN` com as válidas.
pub fn channel_spec(key: &str) -> Result<&'static SettingSpec, ApiError> {
    spec(key)
        .filter(|spec| spec.channel_override)
        .ok_or_else(|| {
            ApiError::new(
                "SETTING_UNKNOWN",
                format!("Configuracao sem sobreposicao por canal: {key}"),
            )
            .with_details(json!({
                "key": key,
                "valid": SETTINGS
                    .iter()
                    .filter(|spec| spec.channel_override)
                    .map(|spec| spec.key)
                    .collect::<Vec<_>>(),
            }))
        })
}

/// Categoria de memória (`categories.list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Category {
    /// Maiúsculas, dígitos e `_`, começando por letra (até 40 caracteres).
    #[schema(example = "CONSTRAINTS")]
    pub key: String,
    #[schema(example = "Restrições")]
    pub label: String,
    /// De -1000 a 1000; maior entra antes no contexto.
    #[schema(example = 30)]
    pub priority: i64,
}

fn valid_category_key(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= MAX_CATEGORY_KEY
        && chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn parse_categories(value: &Value) -> Result<Vec<Category>, String> {
    let categories: Vec<Category> = serde_json::from_value(value.clone())
        .map_err(|_| "uma lista de { key, label, priority }".to_owned())?;
    if categories.is_empty() || categories.len() > MAX_CATEGORIES {
        return Err(format!("de 1 a {MAX_CATEGORIES} categorias"));
    }
    let mut keys = BTreeSet::new();
    for category in &categories {
        if !valid_category_key(&category.key) {
            return Err(format!(
                "key {:?} deve ter maiusculas, digitos e _, comecando por letra (ate {MAX_CATEGORY_KEY})",
                category.key
            ));
        }
        if !keys.insert(category.key.as_str()) {
            return Err(format!("key repetida: {}", category.key));
        }
        let label = category.label.trim().chars().count();
        if label == 0 || label > MAX_CATEGORY_LABEL {
            return Err(format!("label de 1 a {MAX_CATEGORY_LABEL} caracteres"));
        }
        if !(-1000..=1000).contains(&category.priority) {
            return Err("priority de -1000 a 1000".to_owned());
        }
    }
    Ok(categories)
}

/// Confere o valor contra o tipo e o intervalo da chave.
pub fn validate(spec: &SettingSpec, value: &Value) -> Result<(), ApiError> {
    let invalid = |message: String, details: Value| {
        Err(ApiError::new("SETTING_INVALID", message).with_details(details))
    };
    match spec.kind {
        SettingType::Integer { min, max } => match value.as_i64() {
            Some(number) if (min..=max).contains(&number) => Ok(()),
            _ => invalid(
                format!("{} deve ser um inteiro de {min} a {max}", spec.key),
                json!({ "key": spec.key, "type": "integer", "min": min, "max": max }),
            ),
        },
        SettingType::Number { min, max } => match value.as_f64() {
            Some(number) if (min..=max).contains(&number) => Ok(()),
            _ => invalid(
                format!("{} deve ser um numero de {min} a {max}", spec.key),
                json!({ "key": spec.key, "type": "number", "min": min, "max": max }),
            ),
        },
        SettingType::Boolean => match value {
            Value::Bool(_) => Ok(()),
            _ => invalid(
                format!("{} deve ser true ou false", spec.key),
                json!({ "key": spec.key, "type": "boolean" }),
            ),
        },
        SettingType::Categories => match parse_categories(value) {
            Ok(_) => Ok(()),
            Err(reason) => invalid(
                format!("{} invalida: {reason}", spec.key),
                json!({ "key": spec.key, "type": "categories", "reason": reason }),
            ),
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

    /// Valor efetivo de uma chave do catálogo.
    pub fn value(&self, key: &str) -> Value {
        self.values
            .get(key)
            .cloned()
            .or_else(|| spec(key).map(default_value))
            .unwrap_or(Value::Null)
    }

    pub fn categories(&self) -> Vec<Category> {
        self.values
            .get(CATEGORIES)
            .and_then(|value| parse_categories(value).ok())
            .or_else(|| {
                spec(CATEGORIES).and_then(|spec| parse_categories(&default_value(spec)).ok())
            })
            .unwrap_or_default()
    }

    pub fn text_min(&self) -> usize {
        usize::try_from(self.integer(TEXT_MIN)).unwrap_or(usize::MAX)
    }

    /// `memory.text_max` com a sobreposição do canal, quando houver uma válida.
    pub fn text_max(&self, overrides: Option<&BTreeMap<String, Value>>) -> usize {
        let value = overrides
            .and_then(|overrides| overrides.get(TEXT_MAX))
            .filter(|value| spec(TEXT_MAX).is_some_and(|spec| validate(spec, value).is_ok()))
            .and_then(Value::as_i64)
            .unwrap_or_else(|| self.integer(TEXT_MAX));
        usize::try_from(value).unwrap_or(usize::MAX)
    }

    pub fn dedupe_exact(&self) -> bool {
        self.values
            .get(DEDUPE_EXACT)
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    pub fn similar_threshold(&self) -> f64 {
        self.values
            .get(SIMILAR_THRESHOLD)
            .and_then(Value::as_f64)
            .unwrap_or(0.85)
    }
}

/// Uma configuração com sobreposição por canal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelSettingView {
    #[schema(example = "memory.text_max")]
    pub key: String,
    /// Valor efetivo no canal.
    #[schema(value_type = Object, example = 2000)]
    pub value: Value,
    /// Valor global (`get_settings`).
    #[schema(value_type = Object, example = 4000)]
    pub global: Value,
    /// Sobreposição do canal; `null` quando vale o global.
    #[serde(rename = "override")]
    #[schema(value_type = Option<Object>, example = 2000)]
    pub channel_value: Option<Value>,
    #[serde(rename = "type")]
    #[schema(example = "integer")]
    pub kind: String,
    #[schema(value_type = Option<f64>, example = 10)]
    pub min: Option<serde_json::Number>,
    #[schema(value_type = Option<f64>, example = 20000)]
    pub max: Option<serde_json::Number>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelSettingsView {
    #[schema(example = "eng/global")]
    pub channel: String,
    #[schema(example = "ch_1a2b3c4d")]
    pub channel_id: String,
    /// As chaves que aceitam sobreposição por canal.
    pub settings: Vec<ChannelSettingView>,
}

/// Mapa parcial `chave → valor` de sobreposições do canal; `null` remove a sobreposição.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[schema(example = json!({ "memory.text_max": 2000 }))]
pub struct UpdateChannelSettingsBody(pub BTreeMap<String, Value>);

pub fn channel_view(
    snapshot: &SettingsSnapshot,
    channel: &Channel,
    overrides: Option<&BTreeMap<String, Value>>,
) -> ChannelSettingsView {
    let settings = SETTINGS
        .iter()
        .filter(|spec| spec.channel_override)
        .map(|spec| {
            let global = snapshot.value(spec.key);
            let channel_value = overrides
                .and_then(|overrides| overrides.get(spec.key))
                .filter(|value| validate(spec, value).is_ok())
                .cloned();
            let (min, max) = spec.kind.bounds();
            ChannelSettingView {
                key: spec.key.to_owned(),
                value: channel_value.clone().unwrap_or_else(|| global.clone()),
                global,
                channel_value,
                kind: spec.kind.name().to_owned(),
                min,
                max,
                description: spec.description.to_owned(),
            }
        })
        .collect();
    ChannelSettingsView {
        channel: channel.slug.clone(),
        channel_id: channel.id.clone(),
        settings,
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
    /// Mínimo (`integer`, `number`) ou itens mínimos (`categories`); `null` para `boolean`.
    #[schema(value_type = Option<f64>, example = 1)]
    pub min: Option<serde_json::Number>,
    #[schema(value_type = Option<f64>, example = 100000)]
    pub max: Option<serde_json::Number>,
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
            let (min, max) = spec.kind.bounds();
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
    /// `409 CATEGORY_IN_USE` com a contagem das categorias usadas fora de `retained`.
    async fn category_in_use(&self, retained: Vec<String>) -> ApiError {
        let counts = match self.store().category_counts().await {
            Ok(counts) => counts,
            Err(error) => return ApiError::from(error),
        };
        let in_use: BTreeMap<String, u64> = counts
            .into_iter()
            .filter(|(key, _)| !retained.contains(key))
            .collect();
        ApiError::new(
            "CATEGORY_IN_USE",
            "Ha memorias nas categorias retiradas; mova ou recategorize antes",
        )
        .with_details(json!({ "in_use": in_use }))
    }

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
        let mut retained = None;
        for (name, value) in &body.0 {
            let spec = spec(name).ok_or_else(|| unknown(name))?;
            let value_json = if value.is_null() {
                None
            } else {
                validate(spec, value)?;
                Some(value.to_string())
            };
            if spec.key == CATEGORIES {
                let effective = if value.is_null() {
                    default_value(spec)
                } else {
                    value.clone()
                };
                retained = parse_categories(&effective)
                    .ok()
                    .map(|categories| categories.into_iter().map(|c| c.key).collect::<Vec<_>>());
            }
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
                    match core
                        .store()
                        .update_settings(changes, retained.clone(), context)
                        .await
                    {
                        Err(StoreError::Conflict("CATEGORY_IN_USE")) => {
                            Err(core.category_in_use(retained.unwrap_or_default()).await)
                        }
                        other => {
                            other.map_err(|error| super::store_error(error, ApiError::internal))
                        }
                    }
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

impl<S: Store> Core<S> {
    pub async fn get_channel_settings(
        &self,
        principal: &Principal,
        channel: &str,
    ) -> Result<ChannelSettingsView, ApiError> {
        self.authorize(principal, "get_channel_settings")?;
        let network = self.network().await?;
        let channel = super::channels::find_in(&network, channel)?;
        Ok(channel_view(
            &self.settings_snapshot(),
            channel,
            network.channel_settings.get(&channel.id),
        ))
    }

    pub async fn update_channel_settings(
        &self,
        principal: &Principal,
        channel: &str,
        body: UpdateChannelSettingsBody,
        key: Option<&str>,
    ) -> Result<Mutation<ChannelSettingsView>, ApiError> {
        let operation = self.authorize(principal, "update_channel_settings")?;
        if body.0.is_empty() {
            return Err(ApiError::validation("Informe ao menos uma configuracao"));
        }
        let mut changes = Vec::with_capacity(body.0.len());
        for (name, value) in &body.0 {
            let spec = channel_spec(name)?;
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
        super::channels::channel_lookup(channel)?;
        let input = json!({ "channel": channel, "settings": body });
        let core = self.clone();
        let target = channel.to_owned();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |context| async move {
                    let network = core.network().await?;
                    let id = super::channels::find_in(&network, &target)?.id.clone();
                    core.store()
                        .update_channel_settings(id, changes, context)
                        .await
                        .map_err(|error| {
                            super::store_error(error, super::channels::channel_not_found)
                        })
                },
            )
            .await;
        self.network_changed();
        let mutation = result?;
        let network = self.network().await?;
        let channel = super::channels::find_in(&network, channel)?;
        Ok(Mutation {
            value: channel_view(&self.settings_snapshot(), channel, Some(&mutation.value)),
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
    fn numbers_booleans_and_categories_are_validated() {
        let threshold = spec(SIMILAR_THRESHOLD).expect("threshold");
        assert!(validate(threshold, &json!(0)).is_ok());
        assert!(validate(threshold, &json!(0.5)).is_ok());
        assert!(validate(threshold, &json!(1.2)).is_err());
        assert!(validate(threshold, &json!("0.5")).is_err());
        let dedupe = spec(DEDUPE_EXACT).expect("dedupe");
        assert!(validate(dedupe, &json!(false)).is_ok());
        assert!(validate(dedupe, &json!(0)).is_err());
        let categories = spec(CATEGORIES).expect("categories");
        let one = json!([{ "key": "RULES", "label": "Regras", "priority": 1 }]);
        assert!(validate(categories, &one).is_ok());
        for invalid in [
            json!([]),
            json!([{ "key": "rules", "label": "R", "priority": 1 }]),
            json!([{ "key": "A", "label": "", "priority": 1 }]),
            json!([{ "key": "A", "label": "A", "priority": 2000 }]),
            json!([
                { "key": "A", "label": "A", "priority": 1 },
                { "key": "A", "label": "B", "priority": 2 }
            ]),
            json!([{ "key": "A", "label": "A", "priority": 1, "x": 1 }]),
        ] {
            assert_eq!(
                validate(categories, &invalid).expect_err("invalid").code,
                "SETTING_INVALID",
                "{invalid}"
            );
        }
    }

    #[test]
    fn memory_settings_have_typed_readers_and_channel_overrides() {
        let snapshot = SettingsSnapshot::default();
        assert_eq!(snapshot.text_min(), 3);
        assert_eq!(snapshot.text_max(None), 4000);
        assert!(snapshot.dedupe_exact());
        assert!((snapshot.similar_threshold() - 0.85).abs() < 1e-9);
        assert_eq!(snapshot.categories().len(), 5);
        assert_eq!(snapshot.categories()[0].key, "PROJECT_RULES");
        let overrides = BTreeMap::from([(TEXT_MAX.to_owned(), json!(200))]);
        assert_eq!(snapshot.text_max(Some(&overrides)), 200);
        let invalid = BTreeMap::from([(TEXT_MAX.to_owned(), json!(1))]);
        assert_eq!(snapshot.text_max(Some(&invalid)), 4000);
        assert!(channel_spec(TEXT_MAX).is_ok());
        let error = channel_spec(TEXT_MIN).expect_err("global");
        assert_eq!(error.code, "SETTING_UNKNOWN");
        assert_eq!(error.details.expect("details")["valid"], json!([TEXT_MAX]));
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
