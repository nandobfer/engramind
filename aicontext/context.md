# Contexto (`get_context`)

## Onde vive

- `src/core/context/mod.rs`: tipos públicos (`ContextFormat`, `ContextView`, `ContextContent`…), `render` (puro), `Core::get_context`, `Core::context_for_search` (busca) e `Core::channel_titles` (resource do canal).
- `src/core/context/order.rs` (ordem do FR-002), `budget.rs` (duas faixas e teto por canal), `render.rs` (`tagged`, `markdown`, `json` e escape), `version.rs` (digest, `cv_…`, `If-None-Match`), `cache.rs` (LRU de 1024) e `estimator.rs` (`tiktoken-rs`).
- `src/core/counters.rs`: `injected_count` e `usage_log` em lote, com os demais contadores (flush a cada 5 s e no shutdown).
- `src/store/sqlite/context.rs`: `context_memories` (memórias e versões numa transação de leitura), `bump_injections`, `append_usage`. Migração `migrations/0005_usage.sql`.
- Adaptadores: `src/rest/context.rs` (ETag, `Cache-Control`, `Vary`, `304`), ferramenta `get_context` em `src/mcp/tools.rs`, resources e prompt em `src/mcp/resources.rs`, `engramind context` em `src/cli/`.

## Operação

- `get_context` (tag `Context`, qualquer token·read): `GET /v1/context?channels=&format=` com `If-None-Match`; MCP `get_context { channels?, format?, if_none_match? }`; CLI `engramind context [--format] [--if-none-match]`.
- MCP também: resources `engramind://context`, `engramind://channels/{slug}`, `engramind://guides/{id}` e o prompt `memory-writing` (`docs/prompts/memory-writing.md`).

Guias: `contexto-e-cache`, `integracao-consumidor`, `integracao-mcp`.

## Invariantes

- O alcance vem só de `access::resolve(…, Purpose::Context, …)`: canais `inject`, nunca arquivados; `admin` alcança todos os ativos.
- O `304` não toca o banco: a `version` sai da rede em cache e do `SettingsSnapshot`. Não adicione leitura de banco antes da comparação com `If-None-Match`.
- Nada além de uma mudança real (memória, canal, assinatura, configuração de renderização) muda a ordem, o conteúdo ou a `version`. Contadores nunca entram.
- Toda escrita que muda o que o bloco mostra (texto, título, status, categoria, canal) precisa incrementar `channels.version` e chamar `network_changed()`; o Redator também ao aplicar só o título (`Store::apply_redaction`).
- Chave nova que muda o bloco entra em `settings::CONTEXT_RENDER_KEYS` (hash da `version`).
- `importance` só muda junto com `channels.version`: o consolidador grava `importance_pending` e `promote_importance` a promove em toda transação que incrementa a versão do canal (009 D-001, [`consolidation.md`](consolidation.md)).
- A escolha das memórias é a mesma em todos os formatos; o custo é a linha `tagged`.
- O texto vai para o bloco escapado (D-002); nunca para logs. `304` é `DEBUG`.
- `injected_count` e `usage_log` só em respostas com conteúdo; `usage_log.at` é RFC 3339 UTC ordenável.

Desenho: §6, §8, §13. Decisões: `0013` e `0014`. Spec: `specs/005-contexto/`.
