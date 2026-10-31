# Implement - integração

**Data**: 2026-10-31

## Ordem real

1. Revisão do plano contra o código da 0.7.0 (eventos inexistentes, idempotência já limpa em `background.rs`, backoff único da fila, `litestream ltx` por chamada, `/mcp` numa rota só) e Q-001 a Q-015, aprovadas pela usuária com as recomendações (`clarify.md`).
2. Spike T003: `metrics` 0.24 + `metrics-exporter-prometheus` 0.18.3 (sem features padrão) com `OnceLock`; o `OperationSlot` criado pelo middleware de log viaja nos `Parts` que o `rmcp` repassa à ferramenta, então o rótulo `operation` chega por ferramenta MCP sem extensão extra no `Principal` (decisão 0022).
3. Migração `0008_integration.sql`; `App.webhook`, `UpdateAppBody`, segredo cifrado com AAD `webhook:<slug>`; `Store::webhook_directory`/`webhook_target`.
4. `core::events` (`EventDraft`, `recipients`, diretório em cache na geração da rede), `WriteContext.events`, `journal::emit`/`emit_with`; eventos em memórias (`move` com dois), canais, assinaturas, configurações, segredos e Redator; `network_changed()` em apps, agentes e tokens; `jobs_enqueued()` em `Core::mutate`.
5. Job `webhook` (`core::webhooks`), `JobHandler::backoff`, runner com 4 em paralelo, `Core::run_webhook_jobs`.
6. `import_memories` (`core::import`, `Store::import_memories`), `get_usage` (`core::usage_report`, `store/sqlite/integration.rs`), retenção (`core::retention`, `Store::purge_before`), amostrador do backup e `get_status` completo.
7. `src/metrics.rs` e `/metrics`; métricas HTTP no middleware, falhas de autenticação, falhas de jobs, LLM, embeddings, webhooks e retenção.
8. Três vias, tag `Integration`, códigos `IDEMPOTENCY_KEY_REQUIRED` e `BATCH_TOO_LARGE`, 7 configurações, guias, skill, `aicontext/webhooks.md`, desenho, decisões 0022–0024, versão 0.8.0.

## Mudanças além do contrato

- Id do evento com 16 hex (`evt_` + 16), porque o destinatário deduplica por id.
- `CATEGORY_NOT_FOUND` é `400` (como na criação), não `404`.
- `engramind_llm_cost_microusd_total` em vez de `…_cost_usd_total` (contadores inteiros); `engramind_auth_failures_total{reason}` com `missing`/`invalid`/`inactive` (sem revelar expirado vs revogado).
- `update_channel_settings` gera só `settings.updated` (com o canal), não `channel.updated` também.
- Um segredo gravado sem URL vale para a URL informada depois; `Core::purge_idempotency` saiu (a retenção cobre).
- `delete_channel` emite o evento antes de apagar o canal, com a última `channel_version`.

## Comandos de teste

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::/)'
cargo nextest run -E 'test(/webhooks::|integration::|matrix::|contract::/)'
cargo nextest run --locked
```

## Rascunho de release notes

O Engramind avisa: cada app pode receber eventos assinados (HMAC-SHA256) das mudanças nos canais que os agentes dele assinam, e os apps admin recebem tudo, com retentativa e reenvio pela fila. `import_memories` traz memórias de outro sistema preservando datas e autoria. Para operar: `get_status` completo, `get_usage` por dia, agente, operação ou canal, `/metrics` para o Prometheus e retenção automática das tabelas de log.
