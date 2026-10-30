# Contracts - integração

**Criado em**: 2026-10-30
**Status**: Draft
**Feature**: [`spec.md`](spec.md)

## Migração `0008_webhooks.sql`

```sql
ALTER TABLE apps ADD COLUMN webhook_url TEXT NULL;
ALTER TABLE apps ADD COLUMN webhook_secret_enc BLOB NULL;
ALTER TABLE apps ADD COLUMN webhook_secret_updated_at TEXT NULL;
```

## Operações

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `import_memories` | admin·write | `POST /v1/memories/import` | `engramind memories import --channel … --file …` |
| `get_usage` | admin·read | `GET /v1/usage` | `engramind usage` |
| `update_app` (estendida) | admin·write | `PATCH /v1/apps/{app}` com `webhook_url`, `webhook_secret` | `engramind apps update <app> --webhook-url … --webhook-secret-stdin` |

Rota técnica: `GET /metrics` (`get_metrics`, tag Operations, Bearer `admin·read`).

## Webhook

```http
POST <webhook_url>
Content-Type: application/json
Engramind-Event-Id: evt_3f9e1a7c
Engramind-Signature: t=1793370000,v1=<hex>

{ "id": "evt_3f9e1a7c", "type": "memory.created", "channel": "eng/global",
  "channel_version": 42, "memory_ids": ["m_8f2a1c3d"], "at": "2026-10-30T12:00:00Z" }
```

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal |
|---|---|---|---|
| `webhooks.enabled` | booleano | true | não |
| `webhooks.timeout_ms` | inteiro | 5000 | não |
| `webhooks.max_attempts` | inteiro | 5 | não |
| `retention.usage_days` | inteiro | 90 | não |
| `retention.llm_calls_days` | inteiro | 90 | não |
| `retention.audit_days` | inteiro ou null | null | não |

## Códigos de erro novos

| Código | Status |
|---|---|
| `IDEMPOTENCY_KEY_REQUIRED` | 400 |
| `BATCH_TOO_LARGE` | 400 |
