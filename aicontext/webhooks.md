# Integração: eventos, webhooks, importação, uso, métricas e retenção

## Onde vive

- `src/core/events.rs`: `EventType` (7 tipos), `EventDraft`, `recipients` (puro, testado), `DirectoryCache`, `Core::event`/`channel_event`/`subscription_event`/`existing_subscription_event`/`settings_event`.
- `src/core/webhooks.rs`: job `webhook` (`run_webhook`), `signature`, `backoff` (10 s × 4ⁿ, teto 1 h), `WEBHOOK_CONCURRENCY = 4`; runner `Webhook` em `src/core/jobs/runner.rs` (`Core::run_webhook_jobs(now)` nos testes).
- `src/store/sqlite/journal.rs`: `emit`/`emit_with` gravam um job por destinatário na transação; `identities.rs`: `webhook_directory`, `webhook_target`, colunas `webhook_*`.
- `src/core/import.rs` + `Store::import_memories` (`src/store/sqlite/memories.rs`); `src/core/usage_report.rs` (`get_usage`; `usage.rs` é o rate limiter) + `src/store/sqlite/integration.rs`.
- `src/core/status.rs` (`get_status` completo, `BackupSample`, `metrics_snapshot`), `src/core/retention.rs`, `src/core/background.rs` (retenção horária e amostrador do backup a cada 60 s).
- `src/metrics.rs`: recorder (`OnceLock`), `OperationSlot`, helpers de cada métrica e a rota `GET /metrics`.
- Adaptadores: `src/rest/integration.rs` (tag `Integration`), `src/rest/status.rs` (`get_usage`), `src/mcp/tools.rs`, `engramind memories import`, `engramind usage`, `engramind apps create|update --webhook-*`.

## Invariantes

- Toda mutação de memória, canal, assinatura, configuração e segredo monta os eventos no `core` (dentro do closure de `Core::mutate`) e os coloca em `context.events`; o `Store` só chama `journal::emit` no caminho que mudou algo. Operação de escrita nova: gere o evento ou entre na lista `WITHOUT_EVENTS` de `tests/it/webhooks.rs` (e em `contracts.md` da 008).
- Destinatários só por `events::recipients` (mesma regra de `subscription_pattern`), nunca em SQL.
- O payload nunca tem texto, título nem valor de configuração; `last_error` do job só tem status ou classe da falha; logs mostram o host, nunca a URL completa.
- A entrega lê URL e segredo na hora; app removido, desativado ou sem URL → `done`/`skipped`.
- `import_memories` exige `Idempotency-Key` (`IDEMPOTENCY_KEY_REQUIRED`), valida tudo antes de gravar e gera um único `memory.created`.
- `/metrics` exige `admin`·`read` (`Core::metrics_snapshot`); rótulos fechados, nunca slug, id ou URL.
- Retenção só por `Store::purge_before` com o enum `RetentionTable`, em lotes de 5.000.

Guias: `integracao-gestor`, `integracao-consumidor`, `operacao`. Desenho: §6, §12, §17, §20. Decisões: `0022`, `0023`, `0024`.
