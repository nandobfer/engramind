//! Acesso efetivo (desenho §4 e §5, D-002): a única porta de acesso a canais. Nenhum serviço
//! consulta assinaturas por conta própria; todos passam por `resolve` ou `require_channel`.
//!
//! acesso efetivo num canal = escopos do token ∩ acesso da assinatura mais específica.

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU64, Ordering},
};

use serde_json::json;

use super::{
    Core,
    model::{Channel, ChannelReach, Mode, Network, Subscription},
    principal::{Principal, Scope, ScopeSet},
    selection::{ChannelPattern, Selection},
};
use crate::{error::ApiError, store::Store};

/// Para que o alcance é pedido; decide quais modos e canais arquivados entram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// `get_context`: só `inject`, nunca arquivados.
    Context,
    /// Busca e dicas: `inject` e `search`; `manage` e arquivados só quando citados pelo slug.
    Search,
    /// Canal citado ou listagem da rede: qualquer modo.
    Explicit,
}

/// Um canal alcançado pelo token, com o modo, os escopos efetivos e a prioridade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reach {
    pub channel: Channel,
    pub mode: Mode,
    pub scopes: ScopeSet,
    pub priority: i64,
    pub subscription: Option<String>,
    pub target: Option<String>,
}

impl Reach {
    /// O canal com o alcance do token, como `list_channels` e `get_channel` o mostram.
    pub fn view(&self) -> Channel {
        Channel {
            reach: Some(ChannelReach {
                mode: self.mode,
                access: self.scopes.scopes(),
                priority: self.priority,
                subscription: self.subscription.clone(),
                target: self.target.clone(),
            }),
            ..self.channel.clone()
        }
    }
}

/// O alvo da assinatura como padrão.
pub fn subscription_pattern(subscription: &Subscription) -> ChannelPattern {
    let target = subscription.target.as_str();
    if subscription.is_prefix {
        ChannelPattern::Prefix(target.strip_suffix('*').unwrap_or(target).to_owned())
    } else {
        ChannelPattern::Exact(target.to_owned())
    }
}

/// A assinatura mais específica do agente que alcança o canal (FR-005).
pub fn best_subscription<'a>(
    subscriptions: impl IntoIterator<Item = &'a Subscription>,
    slug: &str,
) -> Option<&'a Subscription> {
    subscriptions
        .into_iter()
        .map(|subscription| (subscription_pattern(subscription), subscription))
        .filter(|(pattern, _)| pattern.matches(slug))
        .max_by_key(|(pattern, _)| pattern.specificity())
        .map(|(_, subscription)| subscription)
}

/// Alcance do token no canal, sem seleção nem propósito. `admin` alcança todos os canais com
/// os próprios escopos, em modo `inject` (FR-007).
fn reach_of(network: &Network, principal: &Principal, channel: &Channel) -> Option<Reach> {
    if principal.is_admin() {
        return Some(Reach {
            channel: channel.clone(),
            mode: Mode::Inject,
            scopes: principal.scopes,
            priority: channel.priority,
            subscription: None,
            target: None,
        });
    }
    let agent = principal.agent.as_ref()?;
    let subscription = best_subscription(
        network
            .subscriptions
            .iter()
            .filter(|subscription| subscription.agent.id == agent.id),
        &channel.slug,
    )?;
    let access = subscription
        .access
        .as_deref()
        .and_then(|scopes| ScopeSet::from_scopes(scopes).ok())
        .unwrap_or(principal.scopes);
    Some(Reach {
        channel: channel.clone(),
        mode: subscription.mode,
        scopes: principal.scopes.intersection(access),
        priority: subscription.priority.unwrap_or(channel.priority),
        subscription: Some(subscription.id.clone()),
        target: Some(subscription.target.clone()),
    })
}

/// Canais alcançados, em ordem de slug. `include_archived` traz os arquivados (listagem).
pub fn resolve(
    network: &Network,
    principal: &Principal,
    selection: &Selection,
    purpose: Purpose,
    include_archived: bool,
) -> Vec<Reach> {
    network
        .channels
        .iter()
        .filter(|channel| selection.admits(&channel.slug))
        .filter(|channel| {
            channel.archived_at.is_none()
                || include_archived
                || (purpose != Purpose::Context && selection.cites(&channel.slug))
        })
        .filter_map(|channel| reach_of(network, principal, channel))
        .filter(|reach| match purpose {
            Purpose::Context => reach.mode == Mode::Inject,
            Purpose::Search => reach.mode != Mode::Manage || selection.cites(&reach.channel.slug),
            Purpose::Explicit => true,
        })
        .collect()
}

pub fn not_subscribed(slug: &str) -> ApiError {
    ApiError::new(
        "CHANNEL_NOT_SUBSCRIBED",
        format!("O token nao alcanca o canal {slug}"),
    )
    .with_details(json!({ "channel": slug }))
}

/// Alcance num canal citado explicitamente; fora da seleção ou sem assinatura →
/// `403 CHANNEL_NOT_SUBSCRIBED` (FR-008).
pub fn reach_channel(
    network: &Network,
    principal: &Principal,
    selection: &Selection,
    channel: &Channel,
) -> Result<Reach, ApiError> {
    if !selection.admits(&channel.slug) {
        return Err(not_subscribed(&channel.slug));
    }
    reach_of(network, principal, channel).ok_or_else(|| not_subscribed(&channel.slug))
}

/// `403 SCOPE_REQUIRED` com `details.channel` quando o acesso efetivo não tem o escopo.
pub fn require_scope(reach: &Reach, scope: Scope) -> Result<(), ApiError> {
    if reach.scopes.contains(scope) {
        return Ok(());
    }
    Err(ApiError::new(
        "SCOPE_REQUIRED",
        format!(
            "O acesso efetivo ao canal {} nao inclui o escopo {}",
            reach.channel.slug,
            scope.as_str()
        ),
    )
    .with_details(json!({
        "required_kind": "any",
        "required_scope": scope.as_str(),
        "channel": reach.channel.slug,
    })))
}

/// Cache da rede em memória (D-004, Q-012). Toda mutação de canal ou assinatura incrementa a
/// geração depois do commit; uma leitura guarda a geração lida antes de consultar o banco, então
/// um resultado obsoleto nunca fica marcado como atual.
#[derive(Default)]
pub(crate) struct NetworkCache {
    generation: AtomicU64,
    cached: RwLock<Option<(u64, Arc<Network>)>>,
}

impl NetworkCache {
    fn current(&self, generation: u64) -> Option<Arc<Network>> {
        self.cached
            .read()
            .ok()?
            .as_ref()
            .filter(|(cached, _)| *cached == generation)
            .map(|(_, network)| network.clone())
    }

    fn store(&self, generation: u64, network: Arc<Network>) {
        if let Ok(mut cached) = self.cached.write()
            && cached
                .as_ref()
                .is_none_or(|(current, _)| *current <= generation)
        {
            *cached = Some((generation, network));
        }
    }

    pub(crate) fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}

impl<S: Store> Core<S> {
    /// A rede atual, do cache ou do banco.
    pub(crate) async fn network(&self) -> Result<Arc<Network>, ApiError> {
        let cache = self.network_cache();
        let generation = cache.generation.load(Ordering::Acquire);
        if let Some(network) = cache.current(generation) {
            return Ok(network);
        }
        let network = Arc::new(self.store().network().await?);
        cache.store(generation, network.clone());
        Ok(network)
    }

    /// Canais alcançados pelo token para o propósito, dentro da seleção.
    pub async fn resolve(
        &self,
        principal: &Principal,
        selection: &Selection,
        purpose: Purpose,
    ) -> Result<Vec<Reach>, ApiError> {
        let network = self.network().await?;
        Ok(resolve(&network, principal, selection, purpose, false))
    }

    /// Alcance num canal citado (id `ch_…` ou slug) com o escopo exigido: `404
    /// CHANNEL_NOT_FOUND`, `403 CHANNEL_NOT_SUBSCRIBED` ou `403 SCOPE_REQUIRED` com
    /// `details.channel`.
    pub async fn require_channel(
        &self,
        principal: &Principal,
        selection: &Selection,
        channel: &str,
        scope: Scope,
    ) -> Result<Reach, ApiError> {
        let network = self.network().await?;
        let channel = super::channels::find_in(&network, channel)?;
        let reach = reach_channel(&network, principal, selection, channel)?;
        require_scope(&reach, scope)?;
        Ok(reach)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::principal::{AgentRef, AppRef, TokenKind};

    fn channel(slug: &str) -> Channel {
        Channel {
            id: format!("ch_{:08x}", slug.len()),
            slug: slug.to_owned(),
            name: slug.to_owned(),
            description: None,
            priority: 0,
            version: 1,
            created_at: "2026-10-30T12:00:00.000Z".to_owned(),
            archived_at: None,
            reach: None,
        }
    }

    fn subscription(
        id: &str,
        agent: &str,
        target: &str,
        mode: Mode,
        access: Option<&str>,
    ) -> Subscription {
        Subscription {
            id: id.to_owned(),
            agent: AgentRef {
                id: agent.to_owned(),
                slug: format!("app-a/{agent}"),
            },
            target: target.to_owned(),
            is_prefix: target.ends_with('*'),
            mode,
            access: access.map(|csv| ScopeSet::parse(csv).expect("access").scopes()),
            priority: None,
            created_at: "2026-10-30T12:00:00.000Z".to_owned(),
            reaches: Vec::new(),
        }
    }

    fn principal(kind: TokenKind, agent: Option<&str>, scopes: &str) -> Principal {
        Principal {
            token_id: "tk_00000001".to_owned(),
            app: AppRef {
                id: "app_00000001".to_owned(),
                slug: "app-a".to_owned(),
            },
            agent: agent.map(|agent| AgentRef {
                id: agent.to_owned(),
                slug: format!("app-a/{agent}"),
            }),
            kind,
            scopes: ScopeSet::parse(scopes).expect("scopes"),
        }
    }

    /// O exemplo do §19 do desenho, com os agentes do app-a.
    fn network() -> Network {
        let mut archived = channel("eng/legado");
        archived.archived_at = Some("2026-10-30T13:00:00.000Z".to_owned());
        Network {
            channels: vec![
                channel("assistente/geral"),
                channel("eng/global"),
                archived,
                channel("eng/projeto-atlas"),
                channel("review/revisor"),
            ],
            subscriptions: vec![
                subscription(
                    "sub_1",
                    "assistente",
                    "assistente/geral",
                    Mode::Inject,
                    Some("read,write"),
                ),
                subscription(
                    "sub_2",
                    "desenvolvedor",
                    "eng/global",
                    Mode::Inject,
                    Some("read"),
                ),
                subscription(
                    "sub_3",
                    "desenvolvedor",
                    "eng/projeto-*",
                    Mode::Inject,
                    Some("read"),
                ),
                subscription("sub_4", "revisor", "eng/global", Mode::Inject, Some("read")),
                subscription(
                    "sub_5",
                    "revisor",
                    "review/revisor",
                    Mode::Inject,
                    Some("read"),
                ),
                subscription(
                    "sub_6",
                    "curador",
                    "eng/global",
                    Mode::Manage,
                    Some("read,write"),
                ),
                subscription(
                    "sub_7",
                    "curador",
                    "eng/projeto-*",
                    Mode::Manage,
                    Some("read,write"),
                ),
                subscription(
                    "sub_8",
                    "curador",
                    "review/revisor",
                    Mode::Manage,
                    Some("read,write"),
                ),
                subscription("sub_9", "amplo", "eng/*", Mode::Search, None),
                subscription("sub_10", "amplo", "eng/global", Mode::Inject, Some("read")),
            ],
        }
    }

    fn slugs(reaches: &[Reach]) -> Vec<&str> {
        reaches
            .iter()
            .map(|reach| reach.channel.slug.as_str())
            .collect()
    }

    #[test]
    fn agents_reach_only_their_subscriptions() {
        let network = network();
        let all = Selection::all();
        let developer = principal(TokenKind::Agent, Some("desenvolvedor"), "read,write");
        let reaches = resolve(&network, &developer, &all, Purpose::Explicit, false);
        assert_eq!(slugs(&reaches), ["eng/global", "eng/projeto-atlas"]);
        assert!(reaches.iter().all(|reach| reach.scopes == ScopeSet::READ));

        let reviewer = principal(TokenKind::Agent, Some("revisor"), "read");
        assert_eq!(
            slugs(&resolve(&network, &reviewer, &all, Purpose::Context, false)),
            ["eng/global", "review/revisor"]
        );
        let stranger = principal(TokenKind::Agent, Some("ninguem"), "read");
        assert!(resolve(&network, &stranger, &all, Purpose::Explicit, true).is_empty());
    }

    #[test]
    fn the_most_specific_subscription_wins() {
        let network = network();
        let broad = principal(TokenKind::Agent, Some("amplo"), "read,write,delete");
        let reaches = resolve(
            &network,
            &broad,
            &Selection::all(),
            Purpose::Explicit,
            false,
        );
        assert_eq!(slugs(&reaches), ["eng/global", "eng/projeto-atlas"]);
        let global = &reaches[0];
        assert_eq!((global.mode, global.scopes), (Mode::Inject, ScopeSet::READ));
        assert_eq!(global.subscription.as_deref(), Some("sub_10"));
        let project = &reaches[1];
        assert_eq!(
            (project.mode, project.scopes),
            (Mode::Search, ScopeSet::ALL)
        );
        assert_eq!(project.target.as_deref(), Some("eng/*"));
    }

    #[test]
    fn access_restricts_but_never_widens_the_token() {
        let network = network();
        let assistant = principal(TokenKind::Agent, Some("assistente"), "read");
        let reach = &resolve(
            &network,
            &assistant,
            &Selection::all(),
            Purpose::Context,
            false,
        )[0];
        assert_eq!(
            reach.scopes,
            ScopeSet::READ,
            "a assinatura nao amplia o token"
        );
        let error = require_scope(reach, Scope::Write).expect_err("write");
        assert_eq!(error.code, "SCOPE_REQUIRED");
        assert_eq!(
            error.details.expect("details")["channel"],
            "assistente/geral"
        );
    }

    #[test]
    fn manage_is_reached_only_when_cited() {
        let network = network();
        let curator = principal(TokenKind::Agent, Some("curador"), "read,write");
        let all = Selection::all();
        assert!(resolve(&network, &curator, &all, Purpose::Context, false).is_empty());
        assert!(resolve(&network, &curator, &all, Purpose::Search, false).is_empty());
        assert_eq!(
            resolve(&network, &curator, &all, Purpose::Explicit, false).len(),
            3,
            "a listagem mostra os canais manage"
        );
        let cited = Selection::parse(None, Some("eng/global,eng/projeto-*")).expect("cited");
        let reaches = resolve(&network, &curator, &cited, Purpose::Search, false);
        assert_eq!(slugs(&reaches), ["eng/global"], "prefixo nao cita");
        assert!(resolve(&network, &curator, &cited, Purpose::Context, false).is_empty());
    }

    #[test]
    fn admin_reaches_every_channel_with_its_own_scopes() {
        let network = network();
        let admin = principal(TokenKind::Admin, None, "read");
        let reaches = resolve(&network, &admin, &Selection::all(), Purpose::Context, false);
        assert_eq!(reaches.len(), 4);
        assert!(reaches.iter().all(|reach| reach.mode == Mode::Inject));
        assert!(reaches.iter().all(|reach| reach.scopes == ScopeSet::READ));
        assert_eq!(
            resolve(&network, &admin, &Selection::all(), Purpose::Explicit, true).len(),
            5
        );
    }

    #[test]
    fn archived_channels_leave_context_but_stay_readable_when_cited() {
        let network = network();
        let admin = principal(TokenKind::Admin, None, "read");
        let cited = Selection::parse(None, Some("eng/legado")).expect("cited");
        assert!(resolve(&network, &admin, &cited, Purpose::Context, false).is_empty());
        assert_eq!(
            resolve(&network, &admin, &cited, Purpose::Search, false).len(),
            1
        );
        let archived = &network.channels[2];
        assert!(reach_channel(&network, &admin, &Selection::all(), archived).is_ok());
    }

    #[test]
    fn selection_restricts_and_explicit_channels_outside_it_are_refused() {
        let network = network();
        let developer = principal(TokenKind::Agent, Some("desenvolvedor"), "read");
        let header = Selection::parse(Some("eng/global"), None).expect("header");
        assert_eq!(
            slugs(&resolve(
                &network,
                &developer,
                &header,
                Purpose::Context,
                false
            )),
            ["eng/global"]
        );
        let project = &network.channels[3];
        let error = reach_channel(&network, &developer, &header, project).expect_err("outside");
        assert_eq!(error.code, "CHANNEL_NOT_SUBSCRIBED");
        let review = &network.channels[4];
        let error = reach_channel(&network, &developer, &Selection::all(), review)
            .expect_err("unsubscribed");
        assert_eq!(error.code, "CHANNEL_NOT_SUBSCRIBED");
        assert_eq!(error.details.expect("details")["channel"], "review/revisor");
    }

    #[test]
    fn a_stale_load_never_replaces_a_newer_generation() {
        let cache = NetworkCache::default();
        let old = Arc::new(Network::default());
        cache.store(0, old.clone());
        assert!(cache.current(0).is_some());
        cache.invalidate();
        let generation = cache.generation.load(Ordering::Acquire);
        assert_eq!(generation, 1);
        assert!(cache.current(generation).is_none(), "invalidado");
        cache.store(generation, Arc::new(network()));
        cache.store(0, old);
        assert_eq!(
            cache.current(generation).expect("atual").channels.len(),
            5,
            "a leitura antiga nao sobrescreve a nova"
        );
    }
}
