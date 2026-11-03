//! Evento de consumo (0.12.0, decisão 0034): `memory.consumed` sai das leituras que entregam
//! memórias (`get_context`, `search_memories`, `get_hints`, `get_memory`), um evento por canal,
//! na mesma fila `webhook` dos demais. Não é mudança: nenhuma versão muda e a leitura nunca falha
//! por causa dele. O corpo leva só identificadores; nunca texto, título, `query`, `text` ou `focus`.

use serde_json::{Map, json};

use super::{
    Core, actor,
    events::{Audience, EventType, consumption_id},
    ids,
    model::WriteContext,
    principal::Principal,
};
use crate::store::Store;

/// O que uma leitura entregou de um canal: ids com texto completo e, no contexto, os só-título.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelConsumption {
    /// Slug do canal.
    pub channel: String,
    pub memory_ids: Vec<String>,
    pub title_ids: Vec<String>,
}

/// O grupo do canal em `groups`, criado no fim se ainda não existir.
pub fn slot<'a>(
    groups: &'a mut Vec<ChannelConsumption>,
    channel: &str,
) -> &'a mut ChannelConsumption {
    let index = match groups.iter().position(|group| group.channel == channel) {
        Some(index) => index,
        None => {
            groups.push(ChannelConsumption {
                channel: channel.to_owned(),
                ..ChannelConsumption::default()
            });
            groups.len() - 1
        }
    };
    &mut groups[index]
}

/// Agrupa `(slug, id)` por canal, na ordem da primeira aparição, sem ids repetidos.
pub fn group<'a>(items: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<ChannelConsumption> {
    let mut groups = Vec::new();
    for (channel, id) in items {
        let ids = &mut slot(&mut groups, channel).memory_ids;
        if !ids.iter().any(|existing| existing == id) {
            ids.push(id.to_owned());
        }
    }
    groups
}

/// Leitura que gerou o consumo e o que só o contexto informa.
#[derive(Debug, Clone, Copy)]
pub struct Consumption<'a> {
    pub operation: &'static str,
    pub context_version: Option<&'a str>,
}

impl<S: Store> Core<S> {
    /// Emite um `memory.consumed` por canal com ao menos um id em `memory_ids`. Sem webhooks,
    /// sem destinatário ou sem nada entregue, não grava nada. Falha vira `WARN`, nunca erro.
    pub(crate) async fn consumed(
        &self,
        principal: &Principal,
        consumption: Consumption<'_>,
        groups: &[ChannelConsumption],
    ) {
        if groups.iter().all(|group| group.memory_ids.is_empty()) {
            return;
        }
        let id = consumption_id();
        let mut events = Vec::new();
        for group in groups.iter().filter(|group| !group.memory_ids.is_empty()) {
            let mut extra = Map::new();
            extra.insert("consumption".to_owned(), json!(id));
            extra.insert("operation".to_owned(), json!(consumption.operation));
            extra.insert("app".to_owned(), json!(principal.app.slug));
            extra.insert(
                "agent".to_owned(),
                json!(principal.agent.as_ref().map(|agent| &agent.slug)),
            );
            if let Some(version) = consumption.context_version {
                extra.insert("title_ids".to_owned(), json!(group.title_ids));
                extra.insert("context_version".to_owned(), json!(version));
            }
            match self
                .event(
                    EventType::MemoryConsumed,
                    Some(&group.channel),
                    Audience::Channel(&group.channel),
                    extra,
                )
                .await
            {
                Ok(Some(mut draft)) => {
                    draft.memory_ids = group.memory_ids.clone();
                    events.push(draft);
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        event = "webhooks.consumed_failed",
                        operation = consumption.operation,
                        error = %error.code,
                        "evento de consumo nao montado; a leitura segue"
                    );
                    return;
                }
            }
        }
        if events.is_empty() {
            return;
        }
        let context = WriteContext {
            actor: actor(principal),
            at: ids::timestamp(ids::now()),
            idempotency: None,
            events,
        };
        match self.store().emit_events(context).await {
            Ok(()) => self.jobs_enqueued(),
            Err(error) => tracing::warn!(
                event = "webhooks.consumed_failed",
                operation = consumption.operation,
                error = %error,
                "evento de consumo nao gravado; a leitura segue"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_channel_in_first_appearance_order_without_repeats() {
        let groups = group([
            ("eng/b", "m_3"),
            ("eng/a", "m_1"),
            ("eng/b", "m_2"),
            ("eng/b", "m_3"),
        ]);
        assert_eq!(
            groups,
            [
                ChannelConsumption {
                    channel: "eng/b".to_owned(),
                    memory_ids: vec!["m_3".to_owned(), "m_2".to_owned()],
                    title_ids: Vec::new(),
                },
                ChannelConsumption {
                    channel: "eng/a".to_owned(),
                    memory_ids: vec!["m_1".to_owned()],
                    title_ids: Vec::new(),
                },
            ]
        );
        assert!(group([]).is_empty());
    }
}
