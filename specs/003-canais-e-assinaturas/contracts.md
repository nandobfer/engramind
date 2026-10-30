# Contracts - canais e assinaturas

**Criado em**: 2026-10-30
**Status**: Draft
**Feature**: [`spec.md`](spec.md)

## Migração `0003_channels.sql`

```sql
channels      (id TEXT PK, slug TEXT UNIQUE, name, description, priority INTEGER DEFAULT 0,
               settings_json TEXT DEFAULT '{}', version INTEGER DEFAULT 1,
               created_at, archived_at NULL)
subscriptions (id TEXT PK, agent_id FK→agents ON DELETE CASCADE, target TEXT, is_prefix INTEGER,
               mode CHECK IN ('inject','search','manage'), access TEXT NULL, priority INTEGER NULL,
               created_at, UNIQUE(agent_id, target))
```

Ids `ch_…` e `sub_…` (8 hex). `target` guarda o slug sem o `/*`; `is_prefix` indica o prefixo.

## Tipos

```rust
pub enum Mode { Inject, Search, Manage }
pub struct ChannelSelection(Vec<ChannelPattern>);   // slugs exatos e prefixos
pub enum ChannelPattern { Exact(Slug), Prefix(Slug) }
pub enum Purpose { Context, Search, Explicit }
pub struct Reach { pub channel: ChannelRef, pub mode: Mode, pub scopes: ScopeSet, pub priority: i64 }
```

## `Store` (extensão)

```rust
async fn create_channel(&self, new: NewChannel, audit: AuditEntry, idem: Option<IdemRecord>) -> Result<Channel, StoreError>;
async fn list_channels(&self, filter: ChannelFilter, page: PageRequest) -> Result<Page<Channel>, StoreError>;
async fn subscriptions_for_agent(&self, agent: &AgentId) -> Result<Vec<Subscription>, StoreError>;
async fn active_channels(&self) -> Result<Vec<ChannelRef>, StoreError>;
// … get/update/archive/restore/delete de canais e CRUD de assinaturas
```

## Operações

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `list_channels` | any·read | `GET /v1/channels?channels=&include_archived=` | `engramind channels list` |
| `get_channel` | any·read | `GET /v1/channels/{channel}` | `engramind channels get <canal>` |
| `create_channel` | admin·write | `POST /v1/channels` | `engramind channels create` |
| `update_channel` | admin·write | `PATCH /v1/channels/{channel}` | `engramind channels update <canal>` |
| `archive_channel` | admin·write | `POST /v1/channels/{channel}/archive` | `engramind channels archive <canal>` |
| `restore_channel` | admin·write | `POST /v1/channels/{channel}/restore` | `engramind channels restore <canal>` |
| `delete_channel` | admin·delete | `DELETE /v1/channels/{channel}` | `engramind channels delete <canal>` |
| `list_subscriptions` | any·read | `GET /v1/subscriptions?agent=&channel=` | `engramind subscriptions list` |
| `subscribe` | admin·write | `POST /v1/subscriptions` | `engramind subscriptions add` |
| `update_subscription` | admin·write | `PATCH /v1/subscriptions/{subscription}` | `engramind subscriptions update <id>` |
| `unsubscribe` | admin·write | `DELETE /v1/subscriptions/{subscription}` | `engramind subscriptions remove <id>` |

```jsonc
// POST /v1/subscriptions
{ "agent": "app-a/curador", "target": "eng/projeto-*", "mode": "manage",
  "access": ["read","write"], "priority": null }
```

Nota: `eng/projeto-*` do §19 do desenho é prefixo de **segmento** (`eng/projeto-atlas`)? Ver Q-005 do `clarify.md`.

## Códigos de erro novos

| Código | Status |
|---|---|
| `CHANNEL_NOT_FOUND`, `SUBSCRIPTION_NOT_FOUND` | 404 |
| `CHANNEL_NOT_SUBSCRIBED` | 403 |
| `SUBSCRIPTION_EXISTS`, `CHANNEL_ARCHIVED`, `CHANNEL_NOT_ARCHIVED` | 409 |
