//! Assinaturas (desenho §5): ligam um agente a um canal exato ou a um prefixo, com modo, acesso
//! e prioridade. Só token `admin` assina; o agente vê só as próprias (FR-012).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use super::{
    Core,
    access::subscription_pattern,
    channels::{find_in, priority},
    idempotency::Mutation,
    identities::{next_cursor, scopes},
    ids,
    model::{Mode, NewSubscription, Subscription, SubscriptionPatch, double_option},
    pagination::{Cursor, PageRequest, paginate},
    principal::{Principal, Scope, ScopeSet},
    selection::ChannelPattern,
    store_error,
};
use crate::{error::ApiError, store::Store};

// ---------- entradas ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct ListSubscriptionsQuery {
    /// Só as deste agente (id ou `app/agente`). Para token `agent`, só as próprias.
    pub agent: Option<String>,
    /// Só as que alcançam este canal (id ou slug), exatas ou por prefixo.
    pub channel: Option<String>,
    /// Cursor opaco devolvido em `next_cursor`.
    pub cursor: Option<String>,
    /// Itens por página, de 1 a 200 (padrão 50).
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubscribeBody {
    /// Agente (id ou `app/agente`).
    #[schema(example = "app-a/curador")]
    pub agent: String,
    /// Canal exato existente (`eng/global`) ou prefixo terminado em `*` (`eng/*`,
    /// `eng/projeto-*`), que vale também para canais criados depois.
    #[schema(example = "eng/projeto-*")]
    pub target: String,
    pub mode: Mode,
    /// Restringe o acesso (`read` obrigatório); omitido ou `null` usa os escopos do token.
    #[schema(example = json!(["read", "write"]))]
    pub access: Option<Vec<Scope>>,
    /// Sobrepõe a prioridade do canal, de -1000 a 1000.
    #[schema(minimum = -1000, maximum = 1000)]
    pub priority: Option<i64>,
}

/// Campos omitidos não mudam; `access: null` volta aos escopos do token e `priority: null` à
/// prioridade do canal. Agente e alvo não mudam: cancele e assine de novo.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSubscriptionBody {
    pub mode: Option<Mode>,
    #[serde(default, deserialize_with = "double_option")]
    pub access: Option<Option<Vec<Scope>>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(minimum = -1000, maximum = 1000)]
    pub priority: Option<Option<i64>>,
}

// ---------- saídas ----------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubscriptionList {
    /// Em ordem de criação.
    pub subscriptions: Vec<Subscription>,
    pub next_cursor: Option<String>,
}

// ---------- validação ----------

pub fn subscription_not_found() -> ApiError {
    ApiError::not_found("SUBSCRIPTION_NOT_FOUND", "Assinatura nao encontrada")
}

fn access(values: &[Scope]) -> Result<Vec<Scope>, ApiError> {
    scopes(values).map(ScopeSet::scopes)
}

fn subscription_id(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if ids::is_valid(ids::SUBSCRIPTION_PREFIX, value) {
        Ok(value.to_owned())
    } else {
        Err(subscription_not_found())
    }
}

fn subscription_patch(body: &UpdateSubscriptionBody) -> Result<SubscriptionPatch, ApiError> {
    Ok(SubscriptionPatch {
        mode: body.mode,
        access: match &body.access {
            None => None,
            Some(None) => Some(None),
            Some(Some(values)) => Some(Some(access(values)?)),
        },
        priority: match body.priority {
            None => None,
            Some(None) => Some(None),
            Some(Some(value)) => Some(Some(priority(value)?)),
        },
    })
}

// ---------- operações ----------

impl<S: Store> Core<S> {
    pub async fn list_subscriptions(
        &self,
        principal: &Principal,
        query: ListSubscriptionsQuery,
    ) -> Result<SubscriptionList, ApiError> {
        self.authorize(principal, "list_subscriptions")?;
        let page = PageRequest::new(query.cursor.as_deref(), query.limit)?;
        let requested = query
            .agent
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let agent_id = if principal.is_admin() {
            match requested {
                Some(agent) => Some(self.find_agent(agent).await?.id),
                None => None,
            }
        } else {
            let own = principal.agent.as_ref().ok_or_else(ApiError::internal)?;
            if requested.is_some_and(|agent| agent != own.id && agent != own.slug) {
                return Ok(SubscriptionList {
                    subscriptions: Vec::new(),
                    next_cursor: None,
                });
            }
            Some(own.id.clone())
        };
        let network = self.network().await?;
        let channel = match query
            .channel
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            Some(channel) => Some(find_in(&network, channel)?.slug.clone()),
            None => None,
        };
        let items: Vec<Subscription> = network
            .subscriptions
            .iter()
            .filter(|subscription| {
                agent_id
                    .as_ref()
                    .is_none_or(|agent| &subscription.agent.id == agent)
            })
            .filter(|subscription| {
                channel
                    .as_ref()
                    .is_none_or(|slug| subscription_pattern(subscription).matches(slug))
            })
            .cloned()
            .collect();
        let page = paginate(items, &page, |subscription| Cursor {
            key: subscription.created_at.clone(),
            id: subscription.id.clone(),
        });
        Ok(SubscriptionList {
            next_cursor: next_cursor(&page),
            subscriptions: page.items,
        })
    }

    pub async fn subscribe(
        &self,
        principal: &Principal,
        body: SubscribeBody,
        key: Option<&str>,
    ) -> Result<Mutation<Subscription>, ApiError> {
        let operation = self.authorize(principal, "subscribe")?;
        let target = ChannelPattern::parse(&body.target)?;
        let restricted = body.access.as_deref().map(access).transpose()?;
        let priority = body.priority.map(priority).transpose()?;
        let core = self.clone();
        let agent = body.agent.clone();
        let mode = body.mode;
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &body,
                201,
                |context| async move {
                    let agent = core.find_agent(&agent).await?;
                    let (stored, is_prefix) = target.stored();
                    let new = NewSubscription {
                        agent_id: agent.id,
                        target: stored.to_owned(),
                        is_prefix,
                        mode,
                        access: restricted,
                        priority,
                    };
                    core.store()
                        .create_subscription(new, context)
                        .await
                        .map_err(|error| store_error(error, subscription_not_found))
                },
            )
            .await;
        self.network_changed();
        result
    }

    pub async fn update_subscription(
        &self,
        principal: &Principal,
        subscription: &str,
        body: UpdateSubscriptionBody,
        key: Option<&str>,
    ) -> Result<Mutation<Subscription>, ApiError> {
        let operation = self.authorize(principal, "update_subscription")?;
        let patch = subscription_patch(&body)?;
        let input = json!({ "subscription": subscription, "body": body });
        let id = subscription_id(subscription)?;
        let core = self.clone();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |context| async move {
                    core.store()
                        .update_subscription(id, patch, context)
                        .await
                        .map_err(|error| store_error(error, subscription_not_found))
                },
            )
            .await;
        self.network_changed();
        result
    }

    pub async fn unsubscribe(
        &self,
        principal: &Principal,
        subscription: &str,
        key: Option<&str>,
    ) -> Result<Mutation<Subscription>, ApiError> {
        let operation = self.authorize(principal, "unsubscribe")?;
        let input = json!({ "subscription": subscription });
        let id = subscription_id(subscription)?;
        let core = self.clone();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |context| async move {
                    core.store()
                        .delete_subscription(id, context)
                        .await
                        .map_err(|error| store_error(error, subscription_not_found))
                },
            )
            .await;
        self.network_changed();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_distinguish_omitted_from_null() {
        let body: UpdateSubscriptionBody =
            serde_json::from_value(json!({ "access": null, "priority": 5 })).expect("body");
        let patch = subscription_patch(&body).expect("patch");
        assert_eq!(patch.mode, None);
        assert_eq!(patch.access, Some(None));
        assert_eq!(patch.priority, Some(Some(5)));
        let omitted = subscription_patch(&UpdateSubscriptionBody::default()).expect("empty");
        assert_eq!(omitted, SubscriptionPatch::default());
    }

    #[test]
    fn access_must_include_read() {
        assert!(access(&[Scope::Write]).is_err());
        assert_eq!(
            access(&[Scope::Write, Scope::Read]).expect("rw"),
            [Scope::Read, Scope::Write]
        );
        assert_eq!(
            subscription_id("sub_zz").expect_err("id").code,
            "SUBSCRIPTION_NOT_FOUND"
        );
    }
}
