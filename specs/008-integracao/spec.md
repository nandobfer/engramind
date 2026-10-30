# Spec - integração

**Criado em**: 2026-10-30
**Status**: Draft
**Versão alvo**: 0.8.0 — **marco: migração de consumidor e gestor**
**Input**: Desenho §6 (retenção), §12 (`webhooks`, `retention`), §13 (`import_memories`, `get_usage`), §17, §19, §20.

## Objetivo

O sistema gestor monta a tela completa em tempo real e os consumidores trazem as memórias que já existem; o serviço fica observável em produção.

## Escopo

- Webhooks por app: URL, segredo cifrado, eventos, assinatura HMAC, destinatários, retentativas.
- Enfileiramento de evento em toda mutação (memória, canal, assinatura, configuração).
- `import_memories`.
- `/metrics` com as métricas do §20.
- `get_status` completo e `get_usage`.
- Retenção de `usage_log`, `llm_calls`, `audit_log` e `idempotency`.
- Guias `integracao-consumidor` (final), `integracao-gestor`, `operacao` (final).

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | Tipos: `memory.created`, `memory.updated` (edição, restauração, fixação, movimento, reversão, revisão do Redator), `memory.archived`, `memory.deleted`, `channel.updated`, `subscription.updated`, `settings.updated`. |
| FR-002 | Payload `{ id, type, channel, channel_version, memory_ids, at }`, sem textos; `settings.updated` usa `channel: null` (ou o canal, para configuração por canal). |
| FR-003 | Destinatários: apps com algum agente cuja assinatura alcança o canal afetado e apps com token `admin` ativo; app sem `webhook_url` é ignorado. Calculados no enfileiramento. |
| FR-004 | `POST` com `Content-Type: application/json`, `Engramind-Event-Id`, `Engramind-Signature: t=<unix>,v1=<hex(HMAC-SHA256(segredo, t + "." + corpo))>`; 2xx = entregue. |
| FR-005 | Falha → backoff até `webhooks.max_attempts`, com timeout `webhooks.timeout_ms`; depois `failed` visível em `list_jobs`. Mesmo `id` em todas as tentativas. |
| FR-006 | `move_memory` gera um evento por canal envolvido. |
| FR-007 | `import_memories { channel, memories[] }` (admin·write, até `limits.max_batch`) preserva `created_at`, autor e `origin`, aplica deduplicação e validações, cria revisão 1, enfileira `redact`/`embed` e um único `memory.created` com todos os ids; exige `Idempotency-Key`. |
| FR-008 | `/metrics` exige token `admin` com `read` (D-002). |
| FR-009 | Métricas: latência por operação, taxa de `304` do contexto, tamanho da fila e falhas por tipo, chamadas e custo do Redator, duração dos embeddings, idade da réplica do Litestream, falhas de autenticação. |
| FR-010 | `get_status`: versão, uptime, banco (tamanho, migrações), backup (ligado, idade da réplica), fila por tipo e estado, Redator (chamadas e custo em 24 h), embeddings (modelo, progresso de reindexação), webhooks (falhas em 24 h). |
| FR-011 | `get_usage { since, until, group_by: agent|operation|channel|day }` agrega `usage_log`. |
| FR-012 | Retenção diária: `usage_log` (`retention.usage_days`), `llm_calls` (`retention.llm_calls_days`), `audit_log` (`retention.audit_days`, `null` = sempre), `idempotency` (24 h). |

## Fora do escopo

- Consolidação (009).
- Tempo real para navegador (é do gestor).

## Critérios de sucesso

- Servidor de webhook de teste recebe eventos assinados verificáveis, com retentativa e deduplicação por `id`.
- Importação de 100 memórias em uma chamada, com datas e autoria preservadas e repetição idempotente.
- Prometheus lê `/metrics` com Bearer.
