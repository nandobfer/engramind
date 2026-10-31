# 0024 — Retenção periódica

**Data**: 2026-10-31 · **Etapa**: 008 (0.8.0) · **Status**: aceita

## Contexto

`usage_log`, `llm_calls`, `audit_log` e `jobs` cresciam sem limite; só a idempotência era limpa (a cada hora, em `background.rs`). A fila não tem agendamento, e cada mutação agora cria um job por destinatário de webhook.

## Decisão

- A retenção roda na tarefa periódica existente, a cada hora (Q-007): `Core::retention_tick` só executa se a última execução (`schema_meta['retention.last_run']`, que guarda o relatório) tiver mais de 1 h, então sobrevive a reinícios sem rodar a cada subida.
- `Store::purge_before(tabela, antes_de, 5000)` com um enum fechado de tabelas; lotes de 5.000 linhas por transação, uma tabela por vez, para não segurar o escritor.
- Prazos: `retention.usage_days` (90), `retention.llm_calls_days` (90), `retention.audit_days` (`null` = sempre), `retention.jobs_days` (30, chave nova, Q-008: só `done`/`failed` por `finished_at`) e idempotência de 24 h. Índices `audit_log (at)` e `jobs (state, finished_at)`.
- O relatório aparece em `get_status.retention`; `INFO retention.purged` só quando remove algo; contador `engramind_retention_removed_total{table}`.

## Consequências

- `get_usage` só enxerga o período retido de `usage_log`.
- Substitui a ideia de um job diário na fila (D-003 antigo da 008).
