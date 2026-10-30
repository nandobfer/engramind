# Contracts - canais e assinaturas

**Criado em**: 2026-10-30
**Status**: Implementada
**Feature**: [`spec.md`](spec.md)

## Migração `0003_channels.sql`

```sql
channels      (id TEXT PK, slug TEXT UNIQUE, name, description, priority INTEGER DEFAULT 0,
               settings_json TEXT DEFAULT '{}', version INTEGER DEFAULT 1,
               created_at, archived_at NULL)
subscriptions (id TEXT PK, agent_id FK→agents ON DELETE CASCADE, target TEXT, is_prefix INTEGER,
               mode CHECK IN ('inject','search','manage'), access TEXT NULL, priority INTEGER NULL,
               created_at, UNIQUE(agent_id, target, is_prefix))
```

Ids `ch_…` e `sub_…` (8 hex). `target` guarda o slug exato ou o prefixo sem o `*` final (`eng/` para `eng/*`, `eng/projeto-` para `eng/projeto-*`); `is_prefix` indica o prefixo (D-005).

## Tipos

```rust
pub enum Mode { Inject, Search, Manage }                        // core::model
pub enum ChannelPattern { Exact(String), Prefix(String) }       // core::selection; Prefix sem o `*`
pub struct Selection { lists: Vec<Vec<ChannelPattern>> }        // cabeçalho ∩ channels
pub enum Purpose { Context, Search, Explicit }                  // core::access
pub struct Reach { pub channel: Channel, pub mode: Mode, pub scopes: ScopeSet, pub priority: i64,
                   pub subscription: Option<String>, pub target: Option<String> }
pub struct Network { pub channels: Vec<Channel>, pub subscriptions: Vec<Subscription> }
```

## `Store` (extensão)

```rust
fn network(&self) -> Result<Network, StoreError>;                   // a rede inteira, para o resolvedor e as listas
fn get_channel(&self, lookup: Lookup) -> Result<Option<Channel>, StoreError>;
fn create_channel(&self, new: NewChannel, context: WriteContext) -> Result<Channel, StoreError>;
fn update_channel(&self, id: String, patch: ChannelPatch, context: WriteContext) -> Result<Channel, StoreError>;
fn set_channel_archived(&self, id: String, archived: bool, context: WriteContext) -> Result<Channel, StoreError>;
fn delete_channel(&self, id: String, context: WriteContext) -> Result<Channel, StoreError>;
fn create_subscription(&self, new: NewSubscription, context: WriteContext) -> Result<Subscription, StoreError>;
fn update_subscription(&self, id: String, patch: SubscriptionPatch, context: WriteContext) -> Result<Subscription, StoreError>;
fn delete_subscription(&self, id: String, context: WriteContext) -> Result<Subscription, StoreError>;
```

`WriteContext` leva autoria, instante e idempotência, gravadas na mesma transação (como na 002).

## Operações

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `list_channels` | any·read | `GET /v1/channels?channels=&include_archived=&cursor=&limit=` | `engramind channels list [--include-archived]` (filtro pelo `--channels` global) |
| `get_channel` | any·read | `GET /v1/channels/{channel}` | `engramind channels get <canal>` |
| `create_channel` | admin·write | `POST /v1/channels` | `engramind channels create` |
| `update_channel` | admin·write | `PATCH /v1/channels/{channel}` | `engramind channels update <canal>` |
| `archive_channel` | admin·write | `POST /v1/channels/{channel}/archive` | `engramind channels archive <canal>` |
| `restore_channel` | admin·write | `POST /v1/channels/{channel}/restore` | `engramind channels restore <canal>` |
| `delete_channel` | admin·delete | `DELETE /v1/channels/{channel}` | `engramind channels delete <canal>` |
| `list_subscriptions` | any·read | `GET /v1/subscriptions?agent=&channel=&cursor=&limit=` | `engramind subscriptions list` |
| `subscribe` | admin·write | `POST /v1/subscriptions` | `engramind subscriptions add` |
| `update_subscription` | admin·write | `PATCH /v1/subscriptions/{subscription}` | `engramind subscriptions update <id>` |
| `unsubscribe` | admin·write | `DELETE /v1/subscriptions/{subscription}` | `engramind subscriptions remove <id>` |

```jsonc
// POST /v1/subscriptions
{ "agent": "app-a/curador", "target": "eng/projeto-*", "mode": "manage",
  "access": ["read","write"], "priority": null }
```

Tags OpenAPI: `Channels` (`list_channels`, `get_channel`, `list_subscriptions`) e `Network` (as mutações, admin). `list_channels` e `get_channel` também leem o cabeçalho `Engramind-Channels`.

## Códigos de erro novos

| Código | Status |
|---|---|
| `CHANNEL_NOT_FOUND`, `SUBSCRIPTION_NOT_FOUND` | 404 |
| `CHANNEL_NOT_SUBSCRIBED` | 403 |
| `SUBSCRIPTION_EXISTS`, `CHANNEL_NOT_ARCHIVED` | 409 |

`CHANNEL_ARCHIVED` (escrita em canal arquivado) entra na 004, com as memórias. `SCOPE_REQUIRED` ganha `details.channel` quando a restrição vem da assinatura.
