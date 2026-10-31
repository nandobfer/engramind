//! Eventos de mudança (desenho §17, 008 D-001): o `core` monta o evento e os destinatários
//! antes da escrita; o `Store` cria um job `webhook` por destinatário na mesma transação, só
//! se a mutação mudou algo de fato.

use std::sync::{Arc, RwLock};

use serde_json::{Map, Value, json};

use super::{
    Core, access::subscription_pattern, ids, model::WebhookDirectory, settings::WEBHOOKS_ENABLED,
};
use crate::{error::ApiError, store::Store};

/// Prefixo do id público de um evento.
pub const EVENT_PREFIX: &str = "evt_";

/// `evt_` + 16 hex: o destinatário deduplica por id, então 32 bits seriam pouco.
fn event_id() -> String {
    let high = ids::generate("");
    let low = ids::generate("");
    format!("{EVENT_PREFIX}{high}{low}")
}

/// Tipos de evento (FR-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    MemoryCreated,
    MemoryUpdated,
    MemoryArchived,
    MemoryDeleted,
    ChannelUpdated,
    SubscriptionUpdated,
    SettingsUpdated,
}

impl EventType {
    pub const ALL: [Self; 7] = [
        Self::MemoryCreated,
        Self::MemoryUpdated,
        Self::MemoryArchived,
        Self::MemoryDeleted,
        Self::ChannelUpdated,
        Self::SubscriptionUpdated,
        Self::SettingsUpdated,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MemoryCreated => "memory.created",
            Self::MemoryUpdated => "memory.updated",
            Self::MemoryArchived => "memory.archived",
            Self::MemoryDeleted => "memory.deleted",
            Self::ChannelUpdated => "channel.updated",
            Self::SubscriptionUpdated => "subscription.updated",
            Self::SettingsUpdated => "settings.updated",
        }
    }
}

/// Um evento a gravar. `memory_ids` vazio recebe os ids que o `Store` informar (a memória
/// criada); `channel_version` é lida na transação, depois do incremento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    pub id: String,
    pub kind: EventType,
    /// Slug do canal afetado.
    pub channel: Option<String>,
    pub memory_ids: Vec<String>,
    /// Identificadores extras por tipo (Q-004): `subscription`, `agent`, `target`, `keys`.
    pub extra: Map<String, Value>,
    /// Apps destinatários (ids), ordenados e sem repetição.
    pub recipients: Vec<String>,
}

impl EventDraft {
    /// Corpo do evento, como o destinatário recebe.
    pub fn payload(&self, channel_version: Option<i64>, memory_ids: &[String], at: &str) -> Value {
        let mut event = Map::new();
        event.insert("id".to_owned(), json!(self.id));
        event.insert("type".to_owned(), json!(self.kind.as_str()));
        event.insert("channel".to_owned(), json!(self.channel));
        event.insert("channel_version".to_owned(), json!(channel_version));
        event.insert("memory_ids".to_owned(), json!(memory_ids));
        event.insert("at".to_owned(), json!(at));
        for (key, value) in &self.extra {
            event.insert(key.clone(), value.clone());
        }
        Value::Object(event)
    }
}

/// Para quem vai o evento.
#[derive(Debug, Clone, Copy)]
pub enum Audience<'a> {
    /// Apps com agente cuja assinatura alcança o canal (pelo slug) e os apps admin.
    Channel(&'a str),
    /// O app do agente (assinatura) e os apps admin.
    Agent(&'a str),
    /// Só os apps admin (configuração global).
    Admins,
}

/// Cache do diretório na mesma geração da rede (`network_changed()` invalida os dois).
#[derive(Default)]
pub(crate) struct DirectoryCache {
    cached: RwLock<Option<(u64, Arc<WebhookDirectory>)>>,
}

/// Destinatários de um evento em `now` (FR-003).
pub fn recipients(
    directory: &WebhookDirectory,
    subscriptions: &[super::model::Subscription],
    audience: Audience<'_>,
    now: &str,
) -> Vec<String> {
    let reachable = |app_id: &str| {
        directory
            .apps
            .iter()
            .any(|app| app.id == app_id && app.webhook && !app.disabled)
    };
    let mut apps: Vec<String> = directory
        .admin_tokens
        .iter()
        .filter(|(_, expires_at)| expires_at.as_deref().is_none_or(|at| at > now))
        .map(|(app_id, _)| app_id.clone())
        .collect();
    let agent_app = |agent_id: &str| {
        directory
            .agents
            .iter()
            .find(|agent| agent.id == agent_id && !agent.disabled)
            .map(|agent| agent.app_id.clone())
    };
    match audience {
        Audience::Channel(slug) => apps.extend(
            subscriptions
                .iter()
                .filter(|subscription| subscription_pattern(subscription).matches(slug))
                .filter_map(|subscription| agent_app(&subscription.agent.id)),
        ),
        Audience::Agent(agent_id) => apps.extend(agent_app(agent_id)),
        Audience::Admins => {}
    }
    apps.retain(|app_id| reachable(app_id));
    apps.sort();
    apps.dedup();
    apps
}

impl<S: Store> Core<S> {
    async fn webhook_directory(&self) -> Result<Arc<WebhookDirectory>, ApiError> {
        let generation = self.network_cache().generation();
        let cache = self.directory_cache();
        if let Some(directory) = cache
            .cached
            .read()
            .ok()
            .and_then(|cached| cached.clone())
            .filter(|(cached, _)| *cached == generation)
            .map(|(_, directory)| directory)
        {
            return Ok(directory);
        }
        let directory = Arc::new(self.store().webhook_directory().await?);
        if let Ok(mut cached) = cache.cached.write()
            && cached
                .as_ref()
                .is_none_or(|(current, _)| *current <= generation)
        {
            *cached = Some((generation, directory.clone()));
        }
        Ok(directory)
    }

    /// Um evento com os destinatários atuais; `None` com `webhooks.enabled = false` ou sem
    /// nenhum destinatário (nenhum job).
    pub(crate) async fn event(
        &self,
        kind: EventType,
        channel: Option<&str>,
        audience: Audience<'_>,
        extra: Map<String, Value>,
    ) -> Result<Option<EventDraft>, ApiError> {
        if !self.settings_snapshot().boolean(WEBHOOKS_ENABLED) {
            return Ok(None);
        }
        let directory = self.webhook_directory().await?;
        let network = self.network().await?;
        let recipients = recipients(
            &directory,
            &network.subscriptions,
            audience,
            &ids::timestamp(ids::now()),
        );
        if recipients.is_empty() {
            return Ok(None);
        }
        Ok(Some(EventDraft {
            id: event_id(),
            kind,
            channel: channel.map(str::to_owned),
            memory_ids: Vec::new(),
            extra,
            recipients,
        }))
    }

    /// `subscription.updated` (Q-004): `channel` só com alvo exato; `subscription` vem da
    /// transação quando a assinatura ainda não existe.
    pub(crate) async fn subscription_event(
        &self,
        agent_id: &str,
        target: &str,
        subscription: Option<&str>,
    ) -> Result<Vec<EventDraft>, ApiError> {
        let mut extra = Map::new();
        extra.insert("subscription".to_owned(), json!(subscription));
        extra.insert("agent".to_owned(), json!(agent_id));
        extra.insert("target".to_owned(), json!(target));
        let channel = (!target.ends_with('*')).then_some(target);
        Ok(self
            .event(
                EventType::SubscriptionUpdated,
                channel,
                Audience::Agent(agent_id),
                extra,
            )
            .await?
            .into_iter()
            .collect())
    }

    /// `subscription.updated` de uma assinatura existente (pela rede em cache).
    pub(crate) async fn existing_subscription_event(
        &self,
        id: &str,
    ) -> Result<Vec<EventDraft>, ApiError> {
        let network = self.network().await?;
        match network
            .subscriptions
            .iter()
            .find(|subscription| subscription.id == id)
        {
            Some(subscription) => {
                self.subscription_event(
                    &subscription.agent.id,
                    &subscription.target,
                    Some(&subscription.id),
                )
                .await
            }
            None => Ok(Vec::new()),
        }
    }

    /// `settings.updated` (Q-004): global para os apps admin; por canal, para quem o alcança.
    pub(crate) async fn settings_event(
        &self,
        channel: Option<&str>,
        keys: &[String],
    ) -> Result<Vec<EventDraft>, ApiError> {
        let mut extra = Map::new();
        extra.insert("keys".to_owned(), json!(keys));
        let audience = match channel {
            Some(slug) => Audience::Channel(slug),
            None => Audience::Admins,
        };
        Ok(self
            .event(EventType::SettingsUpdated, channel, audience, extra)
            .await?
            .into_iter()
            .collect())
    }

    /// Evento de um canal (pelo slug), para os apps que o alcançam e os apps admin.
    pub(crate) async fn channel_event(
        &self,
        kind: EventType,
        slug: &str,
    ) -> Result<Vec<EventDraft>, ApiError> {
        Ok(self
            .event(kind, Some(slug), Audience::Channel(slug), Map::new())
            .await?
            .into_iter()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        model::{DirectoryAgent, DirectoryApp, Mode, Subscription},
        principal::AgentRef,
    };

    fn app(id: &str, webhook: bool, disabled: bool) -> DirectoryApp {
        DirectoryApp {
            id: id.to_owned(),
            webhook,
            disabled,
        }
    }

    fn agent(id: &str, app_id: &str, disabled: bool) -> DirectoryAgent {
        DirectoryAgent {
            id: id.to_owned(),
            app_id: app_id.to_owned(),
            disabled,
        }
    }

    fn subscription(agent_id: &str, target: &str) -> Subscription {
        Subscription {
            id: format!("sub_{agent_id}"),
            agent: AgentRef {
                id: agent_id.to_owned(),
                slug: format!("x/{agent_id}"),
            },
            target: target.to_owned(),
            is_prefix: target.ends_with('*'),
            mode: Mode::Inject,
            access: None,
            priority: None,
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            reaches: Vec::new(),
        }
    }

    fn directory() -> WebhookDirectory {
        WebhookDirectory {
            apps: vec![
                app("app_a", true, false),
                app("app_b", true, false),
                app("app_c", false, false),
                app("app_d", true, true),
                app("app_e", true, false),
                app("app_f", true, false),
            ],
            agents: vec![
                agent("ag_a", "app_a", false),
                agent("ag_b", "app_b", false),
                agent("ag_c", "app_c", false),
                agent("ag_d", "app_d", false),
                agent("ag_off", "app_b", true),
            ],
            admin_tokens: vec![
                ("app_e".to_owned(), None),
                (
                    "app_f".to_owned(),
                    Some("2026-01-01T00:00:00.000Z".to_owned()),
                ),
            ],
        }
    }

    const NOW: &str = "2026-06-01T00:00:00.000Z";

    #[test]
    fn channel_events_reach_subscribers_with_webhook_and_active_admins() {
        let subscriptions = vec![
            subscription("ag_a", "eng/global"),
            subscription("ag_b", "eng/*"),
            subscription("ag_c", "eng/global"),
            subscription("ag_d", "eng/global"),
            subscription("ag_off", "eng/global"),
        ];
        let got = recipients(
            &directory(),
            &subscriptions,
            Audience::Channel("eng/global"),
            NOW,
        );
        // app_c sem webhook, app_d desativado, app_f com token admin expirado.
        assert_eq!(got, ["app_a", "app_b", "app_e"]);
        let got = recipients(
            &directory(),
            &subscriptions,
            Audience::Channel("ops/geral"),
            NOW,
        );
        assert_eq!(got, ["app_e"]);
    }

    #[test]
    fn agent_and_admin_audiences() {
        let got = recipients(&directory(), &[], Audience::Agent("ag_a"), NOW);
        assert_eq!(got, ["app_a", "app_e"]);
        let got = recipients(&directory(), &[], Audience::Agent("ag_off"), NOW);
        assert_eq!(got, ["app_e"]);
        let got = recipients(
            &directory(),
            &[],
            Audience::Admins,
            "2025-01-01T00:00:00.000Z",
        );
        assert_eq!(got, ["app_e", "app_f"], "token admin ainda valido");
    }

    #[test]
    fn payload_has_the_fixed_fields_and_the_extras() {
        let mut extra = Map::new();
        extra.insert("keys".to_owned(), json!(["memory.text_max"]));
        let draft = EventDraft {
            id: "evt_1".to_owned(),
            kind: EventType::SettingsUpdated,
            channel: None,
            memory_ids: Vec::new(),
            extra,
            recipients: vec!["app_e".to_owned()],
        };
        let payload = draft.payload(None, &[], "2026-06-01T00:00:00.000Z");
        assert_eq!(
            payload,
            json!({ "id": "evt_1", "type": "settings.updated", "channel": null,
                    "channel_version": null, "memory_ids": [], "at": "2026-06-01T00:00:00.000Z",
                    "keys": ["memory.text_max"] })
        );
    }
}
