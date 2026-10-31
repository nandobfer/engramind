# Contracts - integração

**Criado em**: 2026-10-30
**Atualizado em**: 2026-10-31
**Status**: Aprovado
**Feature**: [`spec.md`](spec.md)

## Migração `0008_integration.sql`

```sql
ALTER TABLE apps ADD COLUMN webhook_url TEXT;
ALTER TABLE apps ADD COLUMN webhook_secret_enc BLOB;
ALTER TABLE apps ADD COLUMN webhook_secret_updated_at TEXT;

-- Retenção por período (FR-014).
CREATE INDEX audit_log_at ON audit_log (at);
CREATE INDEX jobs_finished ON jobs (state, finished_at);
```

`llm_calls_at`, `usage_log_at` e `idempotency_created` já existem.

## Rust

### Eventos (`src/core/events.rs`)

```rust
pub enum EventType { MemoryCreated, MemoryUpdated, MemoryArchived, MemoryDeleted,
                     ChannelUpdated, SubscriptionUpdated, SettingsUpdated }

/// Montado pelo `core` antes da escrita; o `Store` completa `channel_version` na transação.
pub struct EventDraft {
    pub id: String,                 // evt_…
    pub kind: EventType,
    pub channel: Option<EventChannel>, // { id, slug }
    pub memory_ids: Vec<String>,
    pub extra: EventExtra,          // subscription/agent/target ou keys (Q-004)
    pub recipients: Vec<String>,    // app_id
}
```

- `WriteContext` ganha `events: Vec<EventDraft>`; `journal::emit(transaction, context)` grava um job `webhook` por destinatário com `payload = { app_id, event }`, lendo `channels.version` depois do incremento. Toda função do `Store` que chama `journal::audit` numa operação de FR-001 chama `journal::emit`.
- `Core::recipients(channel) -> Vec<app_id>` usa `Network` + `WebhookDirectory` (cache da mesma geração): `{ apps: [{ id, webhook: bool, disabled }], agent_app: {agent_id → app_id}, admin_tokens: [{ app_id, expires_at }] }`, carregado por `Store::webhook_directory()`.
- `apply_redaction` e `set_title_fallback` recebem os destinatários do evento do Redator.

### Job `webhook` (`src/core/webhooks.rs`)

```rust
pub const JOB_KIND: &str = "webhook";
pub const WEBHOOK_CONCURRENCY: usize = 4;
pub fn backoff(attempts: i64) -> Duration; // 10 s × 4^(n-1), teto 1 h
pub fn signature(secret: &[u8], t: i64, body: &[u8]) -> String; // "t=<t>,v1=<hex>"
```

- `JobHandler::lease` = `webhooks.timeout_ms + 30 s`.
- `JobOutcome`: `Done("delivered")`, `Done("skipped")`, `Retry(status|classe)`, `Fail(…)` quando `attempts >= webhooks.max_attempts`.
- O runner passa a receber o backoff do handler (`JobHandler::backoff`, padrão = o do Redator).

### Importação (`src/core/import.rs`)

```rust
pub struct ImportMemoriesBody { pub channel: String, pub memories: Vec<ImportItem> }
pub struct ImportItem {
    pub text: String,
    pub category: Option<String>,
    pub status: Option<MemoryStatus>,   // active | permanent
    pub created_at: Option<String>,
    pub author: Option<ImportAuthor>,   // { app, agent? }
    pub origin: Option<Origin>,
}
pub struct ImportResult { pub created: Vec<ImportCreated>, pub duplicates: Vec<ImportDuplicate> }
```

`Store::import_memories(channel_id, Vec<NewImportedMemory>, WriteContext) -> ImportResult` grava tudo numa transação.

### Métricas (`src/metrics.rs`)

Portado do sap-middleman (`init()` com `OnceLock`, `render`), mais `Core::authorize_metrics(principal)` e o slot de operação (`OperationSlot`, Q-013).

### Retenção (`src/core/retention.rs`)

`Core::run_retention(now) -> RetentionReport { usage_log, llm_calls, audit_log, jobs, idempotency }`; `Store::purge_before(table, before, limit) -> u64` só com tabelas de um enum fechado.

## Operações

| Operação | Tipo·escopo | REST | MCP | CLI |
|---|---|---|---|---|
| `import_memories` | admin·write | `POST /v1/memories/import` | `import_memories` (`idempotency_key` obrigatório) | `engramind memories import --channel … --file …` |
| `get_usage` | admin·read | `GET /v1/usage` | `get_usage` | `engramind usage --group-by …` |
| `create_app`, `update_app` (estendidas) | admin·write | `POST /v1/apps`, `PATCH /v1/apps/{app}` com `webhook_url`, `webhook_secret` | mesmos argumentos | `engramind apps create|update … --webhook-url … --webhook-secret-stdin` |

- `import_memories`: tag `Integration`, `destructive = false`, `idempotent = true` (exige chave).
- `get_usage`: tag `Status`, `read(...)`.
- Rota técnica `GET /metrics` (`get_metrics`, tag `Operations`, Bearer `admin`·`read`, `text/plain; version=0.0.4`).

## Corpos e respostas

### `POST /v1/memories/import`

```json
{ "channel": "eng/global",
  "memories": [
    { "text": "Rode só os testes afetados.", "category": "CONSTRAINTS",
      "created_at": "2026-01-10T12:00:00Z",
      "author": { "app": "legado", "agent": "curador" },
      "origin": { "source": "import", "label": "magic-context#42" } } ] }
```

`201`:

```json
{ "created": [{ "index": 0, "id": "m_8f2a1c3d" }],
  "duplicates": [{ "index": 3, "existing_id": "m_1b7d0e44" }] }
```

Erros: `400 IDEMPOTENCY_KEY_REQUIRED`, `400 BATCH_TOO_LARGE` (`details: { max, received }`), `400 VALIDATION_ERROR` (`details.index`, `details.field`), `400 CATEGORY_NOT_FOUND` e `404 APP_NOT_FOUND`/`AGENT_NOT_FOUND`/`CHANNEL_NOT_FOUND` (com `details.index`), `409 CHANNEL_ARCHIVED`, `409 IDEMPOTENCY_KEY_CONFLICT`.

### `GET /v1/usage?since=…&until=…&group_by=agent`

```json
{ "since": "2026-10-24T00:00:00.000Z", "until": "2026-10-31T00:00:00.000Z", "group_by": "agent",
  "rows": [{ "key": "ag_3f9e1a7c", "label": "app-a/desenvolvedor",
             "requests": 120, "memories": 2400, "rendered_tokens": 380000 }],
  "total": { "requests": 120, "memories": 2400, "rendered_tokens": 380000 } }
```

### App com webhook

```json
{ "id": "app_3f9e1a7c", "slug": "gestor", "…": "…",
  "webhook": { "url": "https://gestor.interno/engramind/events",
               "secret": { "configured": true, "updated_at": "2026-10-31T12:00:00.000Z" } } }
```

`webhook: null` sem URL.

### `get_status` (acréscimos)

```json
{ "uptime_seconds": 86400,
  "database": { "status": "ok", "migrations": { "applied": 8, "expected": 8 }, "size_bytes": 10485760 },
  "backup": { "enabled": true, "status": "ok", "detail": "…", "replica_age_seconds": 4,
              "last_replica_at": "2026-10-31T11:59:56Z", "checked_at": "2026-10-31T12:00:00Z" },
  "redator": { "…": "…", "last_24h": { "calls": 40, "failures": 1, "cost_usd": 0.0123 } },
  "webhooks": { "enabled": true, "apps": 2, "pending": 0, "delivered_24h": 310, "failed_24h": 0 },
  "retention": { "last_run_at": "2026-10-31T11:00:00Z", "removed": { "usage_log": 0, "llm_calls": 0, "audit_log": 0, "jobs": 120, "idempotency": 3 } } }
```

## Webhook

```http
POST <webhook_url>
Content-Type: application/json
User-Agent: engramind/0.8.0
Engramind-Event-Id: evt_3f9e1a7c0b2d4e6f
Engramind-Event-Type: memory.created
Engramind-Signature: t=1793370000,v1=<hex>

{ "id": "evt_3f9e1a7c0b2d4e6f", "type": "memory.created", "channel": "eng/global",
  "channel_version": 42, "memory_ids": ["m_8f2a1c3d"], "at": "2026-10-30T12:00:00.000Z" }
```

`t` é o instante da tentativa; `at` é o da mutação. O guia recomenda tolerância de 5 min e descarte por `id`.

## Métricas

| Nome | Tipo | Rótulos |
|---|---|---|
| `http_requests_total` | counter | `method`, `route`, `status`, `operation` |
| `http_request_duration_seconds` | histogram | `method`, `route`, `operation` |
| `service_ready` | gauge | — |
| `engramind_build_info` | gauge (1) | `version` |
| `engramind_jobs` | gauge | `kind`, `state` |
| `engramind_jobs_failed_total` | counter | `kind` |
| `engramind_llm_calls_total` | counter | `purpose`, `provider`, `outcome` |
| `engramind_llm_tokens_total` | counter | `purpose`, `direction` (`input`/`output`) |
| `engramind_llm_cost_microusd_total` | counter | `purpose`, `provider` (micro-dólares: contadores são inteiros) |
| `engramind_embedding_duration_seconds` | histogram | `provider`, `kind` (`query`/`document`) |
| `engramind_embedding_failures_total` | counter | `provider`, `kind` |
| `engramind_webhook_deliveries_total` | counter | `outcome` (`delivered`/`retry`/`failed`/`skipped`) |
| `engramind_auth_failures_total` | counter | `reason` (`missing`/`invalid`/`inactive`; não diferencia expirado de revogado para não revelar o estado) |
| `engramind_backup_replica_age_seconds` | gauge | — (ausente sem backup) |
| `engramind_database_size_bytes` | gauge | — |
| `engramind_retention_removed_total` | counter | `table` |

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal | Validação |
|---|---|---|---|---|
| `webhooks.enabled` | booleano | true | não | — |
| `webhooks.timeout_ms` | inteiro | 5000 | não | 500–30000 |
| `webhooks.max_attempts` | inteiro | 5 | não | 1–10 |
| `retention.usage_days` | inteiro | 90 | não | 1–3650 |
| `retention.llm_calls_days` | inteiro | 90 | não | 1–3650 |
| `retention.audit_days` | inteiro ou null | null | não | 1–3650 ou null |
| `retention.jobs_days` | inteiro | 30 | não | 1–3650 (Q-008) |

## Códigos de erro novos

| Código | Status | Saída da CLI |
|---|---|---|
| `IDEMPOTENCY_KEY_REQUIRED` | 400 | validação |
| `BATCH_TOO_LARGE` | 400 | validação |

## Exceções do teste de cobertura de eventos

Operações de escrita sem evento: `create_app`, `update_app`, `delete_app`, `create_agent`, `update_agent`, `delete_agent`, `issue_token`, `update_token`, `revoke_token` (Q-014), `test_redator`, `test_embedding`, `list_embedding_models`, `reembed_all`, `retry_job`.
