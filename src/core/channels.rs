//! Canais (desenho §5): criação e manutenção só por token `admin`; leitura por qualquer token,
//! limitada ao que ele alcança (FR-011).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use super::{
    Core,
    access::{self, Purpose, Reach},
    idempotency::Mutation,
    identities::{MAX_DESCRIPTION, MAX_NAME, next_cursor, optional_text, text},
    ids,
    model::{Channel, ChannelPatch, ChannelStats, Lookup, Network, NewChannel, double_option},
    pagination::{Cursor, PageRequest, paginate},
    principal::{Principal, Scope},
    selection::{Selection, validate_channel_slug},
    store_error,
};
use crate::{error::ApiError, store::Store};

pub const MIN_PRIORITY: i64 = -1000;
pub const MAX_PRIORITY: i64 = 1000;

// ---------- entradas ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct ListChannelsQuery {
    /// Slugs e prefixos (`eng/global,eng/projeto-*`) separados por vírgula; restringe dentro do
    /// que o token alcança e do cabeçalho `Engramind-Channels`.
    pub channels: Option<String>,
    /// Inclui os canais arquivados.
    pub include_archived: Option<bool>,
    /// Cursor opaco devolvido em `next_cursor`.
    pub cursor: Option<String>,
    /// Itens por página, de 1 a 200 (padrão 50).
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateChannelBody {
    /// Segmentos `[a-z0-9-]` separados por `/` (até 8 segmentos e 120 caracteres); único e
    /// imutável.
    #[schema(example = "eng/global")]
    pub slug: String,
    #[schema(example = "Engenharia global")]
    pub name: String,
    pub description: Option<String>,
    /// Prioridade padrão na ordem do contexto, de -1000 a 1000 (padrão 0).
    #[schema(minimum = -1000, maximum = 1000)]
    pub priority: Option<i64>,
}

/// Campos omitidos não mudam; `description: null` limpa. O slug não muda (Q-006).
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateChannelBody {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    /// De -1000 a 1000.
    #[schema(minimum = -1000, maximum = 1000)]
    pub priority: Option<i64>,
}

// ---------- saídas ----------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChannelList {
    /// Em ordem de slug, cada um com o `reach` do token.
    pub channels: Vec<Channel>,
    pub next_cursor: Option<String>,
}

// ---------- validação ----------

pub fn channel_not_found() -> ApiError {
    ApiError::not_found("CHANNEL_NOT_FOUND", "Canal nao encontrado")
}

pub(crate) fn priority(value: i64) -> Result<i64, ApiError> {
    if (MIN_PRIORITY..=MAX_PRIORITY).contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::validation(format!(
            "priority deve estar entre {MIN_PRIORITY} e {MAX_PRIORITY}"
        )))
    }
}

/// `ch_…` é id; o resto precisa ser um slug válido.
pub fn channel_lookup(value: &str) -> Result<Lookup, ApiError> {
    let value = value.trim();
    if ids::is_valid(ids::CHANNEL_PREFIX, value) {
        return Ok(Lookup::Id(value.to_owned()));
    }
    validate_channel_slug(value)?;
    Ok(Lookup::Slug(value.to_owned()))
}

/// Canal da rede por id ou slug.
pub(crate) fn find_in<'a>(network: &'a Network, value: &str) -> Result<&'a Channel, ApiError> {
    let lookup = channel_lookup(value)?;
    network
        .channels
        .iter()
        .find(|channel| match &lookup {
            Lookup::Id(id) => &channel.id == id,
            Lookup::Slug(slug) => &channel.slug == slug,
        })
        .ok_or_else(channel_not_found)
}

fn channel_patch(body: &UpdateChannelBody) -> Result<ChannelPatch, ApiError> {
    Ok(ChannelPatch {
        name: body
            .name
            .as_deref()
            .map(|name| text("name", name, MAX_NAME))
            .transpose()?,
        description: match &body.description {
            None => None,
            Some(value) => Some(optional_text(
                "description",
                value.as_deref(),
                MAX_DESCRIPTION,
            )?),
        },
        priority: body.priority.map(priority).transpose()?,
    })
}

// ---------- operações ----------

impl<S: Store> Core<S> {
    async fn find_channel(&self, value: &str) -> Result<Channel, ApiError> {
        self.store()
            .get_channel(channel_lookup(value)?)
            .await?
            .ok_or_else(channel_not_found)
    }

    pub async fn list_channels(
        &self,
        principal: &Principal,
        query: ListChannelsQuery,
        header: Option<&str>,
    ) -> Result<ChannelList, ApiError> {
        self.authorize(principal, "list_channels")?;
        let selection = Selection::parse(header, query.channels.as_deref())?;
        let page = PageRequest::new(query.cursor.as_deref(), query.limit)?;
        let network = self.network().await?;
        let reaches = access::resolve(
            &network,
            principal,
            &selection,
            Purpose::Explicit,
            query.include_archived.unwrap_or(false),
        );
        let page = paginate(
            reaches.iter().map(Reach::view).collect(),
            &page,
            |channel| Cursor {
                key: channel.slug.clone(),
                id: channel.id.clone(),
            },
        );
        Ok(ChannelList {
            next_cursor: next_cursor(&page),
            channels: page.items,
        })
    }

    pub async fn get_channel(
        &self,
        principal: &Principal,
        channel: &str,
        header: Option<&str>,
    ) -> Result<Channel, ApiError> {
        self.authorize(principal, "get_channel")?;
        let selection = Selection::parse(header, None)?;
        let network = self.network().await?;
        let channel = find_in(&network, channel)?;
        Ok(access::reach_channel(&network, principal, &selection, channel)?.view())
    }

    /// Contagens das memórias do canal; exige `read` efetivo nele.
    pub async fn channel_stats(
        &self,
        principal: &Principal,
        channel: &str,
        header: Option<&str>,
    ) -> Result<ChannelStats, ApiError> {
        self.authorize(principal, "channel_stats")?;
        let selection = Selection::parse(header, None)?;
        let reach = self
            .require_channel(principal, &selection, channel, Scope::Read)
            .await?;
        self.store()
            .channel_stats(reach.channel.id)
            .await?
            .ok_or_else(channel_not_found)
    }

    pub async fn create_channel(
        &self,
        principal: &Principal,
        body: CreateChannelBody,
        key: Option<&str>,
    ) -> Result<Mutation<Channel>, ApiError> {
        let operation = self.authorize(principal, "create_channel")?;
        let slug = body.slug.trim().to_owned();
        validate_channel_slug(&slug)?;
        let new = NewChannel {
            slug,
            name: text("name", &body.name, MAX_NAME)?,
            description: optional_text(
                "description",
                body.description.as_deref(),
                MAX_DESCRIPTION,
            )?,
            priority: priority(body.priority.unwrap_or(0))?,
        };
        let core = self.clone();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &body,
                201,
                |context| async move {
                    core.store()
                        .create_channel(new, context)
                        .await
                        .map_err(|error| store_error(error, channel_not_found))
                },
            )
            .await;
        self.network_changed();
        result
    }

    pub async fn update_channel(
        &self,
        principal: &Principal,
        channel: &str,
        body: UpdateChannelBody,
        key: Option<&str>,
    ) -> Result<Mutation<Channel>, ApiError> {
        let operation = self.authorize(principal, "update_channel")?;
        let patch = channel_patch(&body)?;
        channel_lookup(channel)?;
        let input = json!({ "channel": channel, "body": body });
        let core = self.clone();
        let channel = channel.to_owned();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |context| async move {
                    let id = core.find_channel(&channel).await?.id;
                    core.store()
                        .update_channel(id, patch, context)
                        .await
                        .map_err(|error| store_error(error, channel_not_found))
                },
            )
            .await;
        self.network_changed();
        result
    }

    pub async fn archive_channel(
        &self,
        principal: &Principal,
        channel: &str,
        key: Option<&str>,
    ) -> Result<Mutation<Channel>, ApiError> {
        self.set_archived(principal, "archive_channel", channel, true, key)
            .await
    }

    pub async fn restore_channel(
        &self,
        principal: &Principal,
        channel: &str,
        key: Option<&str>,
    ) -> Result<Mutation<Channel>, ApiError> {
        self.set_archived(principal, "restore_channel", channel, false, key)
            .await
    }

    async fn set_archived(
        &self,
        principal: &Principal,
        name: &str,
        channel: &str,
        archived: bool,
        key: Option<&str>,
    ) -> Result<Mutation<Channel>, ApiError> {
        let operation = self.authorize(principal, name)?;
        channel_lookup(channel)?;
        let input = json!({ "channel": channel });
        let core = self.clone();
        let channel = channel.to_owned();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |context| async move {
                    let id = core.find_channel(&channel).await?.id;
                    core.store()
                        .set_channel_archived(id, archived, context)
                        .await
                        .map_err(|error| store_error(error, channel_not_found))
                },
            )
            .await;
        self.network_changed();
        result
    }

    pub async fn delete_channel(
        &self,
        principal: &Principal,
        channel: &str,
        key: Option<&str>,
    ) -> Result<Mutation<Channel>, ApiError> {
        let operation = self.authorize(principal, "delete_channel")?;
        channel_lookup(channel)?;
        let input = json!({ "channel": channel });
        let core = self.clone();
        let channel = channel.to_owned();
        let result = self
            .mutate(
                principal,
                operation,
                key,
                &input,
                200,
                |context| async move {
                    let id = core.find_channel(&channel).await?.id;
                    core.store()
                        .delete_channel(id, context)
                        .await
                        .map_err(|error| store_error(error, channel_not_found))
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
    fn lookups_distinguish_ids_from_slugs() {
        assert_eq!(
            channel_lookup("ch_0a1b2c3d").expect("id"),
            Lookup::Id("ch_0a1b2c3d".to_owned())
        );
        assert_eq!(
            channel_lookup(" eng/global ").expect("slug"),
            Lookup::Slug("eng/global".to_owned())
        );
        assert_eq!(
            channel_lookup("eng/*").expect_err("prefixo").code,
            "VALIDATION_ERROR"
        );
    }

    #[test]
    fn priority_is_bounded() {
        assert!(priority(MIN_PRIORITY).is_ok());
        assert!(priority(MAX_PRIORITY).is_ok());
        assert!(priority(MAX_PRIORITY + 1).is_err());
        assert!(priority(MIN_PRIORITY - 1).is_err());
    }
}
