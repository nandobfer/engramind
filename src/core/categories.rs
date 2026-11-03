//! Categorias de memória (0.11.0, item 7): `list_categories` para qualquer token com `read` e
//! `create_category` para qualquer token com `write`, que só acrescenta a `categories.list`
//! (nunca sobrescreve). Editar e remover continua no `update_settings` do admin.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use super::{
    Core,
    idempotency::Mutation,
    model::{CategoryAppend, created_default},
    principal::Principal,
    settings::{
        self, CATEGORIES, Category, MAX_CATEGORIES, MAX_CATEGORY_KEY, MAX_CATEGORY_LABEL,
        SettingsSnapshot, valid_category_key,
    },
    store_error,
};
use crate::{error::ApiError, store::Store, store::StoreError};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCategoryBody {
    /// Maiúsculas, dígitos e `_`, começando por letra (até 40 caracteres).
    #[schema(example = "QUESTS")]
    pub key: String,
    /// De 1 a 80 caracteres.
    #[schema(example = "Missões")]
    pub label: String,
    /// De -1000 a 1000; maior entra antes no contexto.
    #[schema(example = 25)]
    pub priority: i64,
}

/// Resposta de `create_category`: a categoria e se ela nasceu agora (`false` na repetição
/// equivalente, `200`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CategoryWrite {
    #[serde(flatten)]
    pub category: Category,
    #[serde(default = "created_default")]
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CategoryList {
    /// Na ordem de `categories.list`; a primeira é a padrão.
    pub categories: Vec<Category>,
}

fn category(body: &CreateCategoryBody) -> Result<Category, ApiError> {
    let key = body.key.trim().to_owned();
    if !valid_category_key(&key) {
        return Err(ApiError::validation(format!(
            "key deve ter maiusculas, digitos e _, comecando por letra (ate {MAX_CATEGORY_KEY})"
        ))
        .with_details(json!({ "field": "key" })));
    }
    let label = body.label.trim().to_owned();
    if !(1..=MAX_CATEGORY_LABEL).contains(&label.chars().count()) {
        return Err(ApiError::validation(format!(
            "label deve ter de 1 a {MAX_CATEGORY_LABEL} caracteres"
        ))
        .with_details(json!({ "field": "label" })));
    }
    if !(-1000..=1000).contains(&body.priority) {
        return Err(
            ApiError::validation("priority deve estar entre -1000 e 1000")
                .with_details(json!({ "field": "priority" })),
        );
    }
    Ok(Category {
        key,
        label,
        priority: body.priority,
    })
}

impl<S: Store> Core<S> {
    pub async fn list_categories(&self, principal: &Principal) -> Result<CategoryList, ApiError> {
        self.authorize(principal, "list_categories")?;
        Ok(CategoryList {
            categories: self.settings_snapshot().categories(),
        })
    }

    pub async fn create_category(
        &self,
        principal: &Principal,
        body: CreateCategoryBody,
        key: Option<&str>,
    ) -> Result<Mutation<CategoryWrite>, ApiError> {
        let operation = self.authorize(principal, "create_category")?;
        let wanted = category(&body)?;
        let default_json = settings::spec(CATEGORIES)
            .map(|spec| spec.default.to_owned())
            .ok_or_else(ApiError::internal)?;
        let core = self.clone();
        let mutation = self
            .mutate(
                principal,
                operation,
                key,
                &body,
                201,
                |mut context| async move {
                    context.events = core.settings_event(None, &[CATEGORIES.to_owned()]).await?;
                    let outcome = core
                        .store()
                        .create_category(wanted.clone(), default_json, MAX_CATEGORIES, context)
                        .await;
                    match outcome {
                        Ok(CategoryAppend::Created) => Ok(CategoryWrite {
                            category: wanted,
                            created: true,
                        }),
                        Ok(CategoryAppend::Existing(existing)) if existing == wanted => {
                            Ok(CategoryWrite {
                                category: existing,
                                created: false,
                            })
                        }
                        Ok(CategoryAppend::Existing(existing)) => Err(ApiError::new(
                            "CATEGORY_CONFLICT",
                            format!(
                                "A categoria {} ja existe com outro label ou prioridade",
                                existing.key
                            ),
                        )
                        .with_details(json!({ "existing": existing }))),
                        Err(StoreError::Conflict("CATEGORY_LIMIT_REACHED")) => Err(ApiError::new(
                            "CATEGORY_LIMIT_REACHED",
                            format!("{CATEGORIES} ja tem {MAX_CATEGORIES} categorias"),
                        )
                        .with_details(json!({ "max": MAX_CATEGORIES }))),
                        Err(error) => Err(store_error(error, ApiError::internal)),
                    }
                },
            )
            .await?;
        if mutation.value.created && !mutation.replayed {
            let rows = self.store().get_settings().await?;
            self.replace_settings(SettingsSnapshot::from_rows(&rows));
            self.network_changed();
        }
        Ok(mutation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_validated_like_categories_list() {
        let body = |key: &str, label: &str, priority: i64| CreateCategoryBody {
            key: key.to_owned(),
            label: label.to_owned(),
            priority,
        };
        assert_eq!(
            category(&body(" QUESTS ", " Missões ", 25)).expect("ok"),
            Category {
                key: "QUESTS".to_owned(),
                label: "Missões".to_owned(),
                priority: 25
            }
        );
        for invalid in [
            body("quests", "M", 1),
            body("1X", "M", 1),
            body(&"A".repeat(41), "M", 1),
            body("A", "", 1),
            body("A", &"x".repeat(81), 1),
            body("A", "M", 1001),
            body("A", "M", -1001),
        ] {
            assert_eq!(
                category(&invalid).expect_err("invalid").code,
                "VALIDATION_ERROR"
            );
        }
    }
}
