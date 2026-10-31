# Implement - contexto

**Data**: 2026-10-30

## Ordem real

1. Planejamento: Q-001 a Q-003 resolvidas pelas recomendações e Q-004 a Q-023 abertas na leitura do código, todas confirmadas pela usuária; na Q-014, a pedido dela, `usage_log.at` ordenável e índices por período (`clarify.md`, D-005 a D-008).
2. Dependência `tiktoken-rs` 0.12 (decisão 0014) e `core::context::estimator` com o trait `TokenEstimator`.
3. Configurações: tipo `enum` (`SettingType::Choice`, `choices` em `SettingView`), as cinco chaves `context.*` (duas por canal), leitores no `SettingsSnapshot` e `CONTEXT_RENDER_KEYS`.
4. Migração `0005_usage.sql` e `src/store/sqlite/context.rs`: `context_memories` numa transação de leitura, `bump_injections` e `append_usage`.
5. Núcleo puro: `order` (FR-002), `budget` (Q-001/Q-002), `render` (três formatos e escape D-002), `version` (digest, `cv_…`, `If-None-Match`) e `cache` (LRU de 1024).
6. `Core::get_context`: alcance pela rede em cache, `304` antes de qualquer leitura, cache, leitura consistente com a `version` recalculada, renderização em `spawn_blocking`, `injected_count` e `usage_log`.
7. `search_memories`: `exclude_context`, `context_version` e a linha de `usage_log`; `counters` com os lotes novos.
8. Catálogo (`get_context`, tag `Context`), REST `GET /v1/context` (ETag, `Cache-Control`, `Vary`, `304`), log do `304` em `DEBUG`, ferramenta MCP com `if_none_match`, resources e prompt (`src/mcp/resources.rs`, capacidades `resources`/`prompts`), erros `RESOURCE_NOT_FOUND`/`PROMPT_NOT_FOUND`, CLI `engramind context` e `memories search --exclude-context`; `serve` aquece o estimador.
9. Testes: unitários de `order`, `budget`, `render`, `version`, `cache`, `estimator`, `settings`, `counters` e os snapshots do §8; `tests/it/context.rs` (11 testes); log do `304` em `tests/it/logging.rs`; matriz; contrato dos argumentos MCP (`If-None-Match` → `if_none_match`), das configurações `enum` e dos resources/prompt documentados; OpenAPI regenerado.
10. Documentação: guias `contexto-e-cache` e `integracao-consumidor`; `integracao-mcp`, `configuracoes`, `cli`, `erros`, `escrever-memorias`, `overview`; skill; `docs/prompts/memory-writing.md`; `aicontext/context.md`, `README.md`, `settings.md`, `memories.md`; decisões `0013` e `0014`; desenho §6, §8 e §13; FR-007 da 006; versão 0.5.0 e `CHANGELOG.md`.

## Mudanças além do contrato

- `usage_log` com `app_id` e índices `(agent_id, at)` e `(op, at)` além de `(at)`.
- `injected_count`/`usage_log` em `core/counters.rs`, porque `core/usage.rs` já é o rate limit (D-005).
- Snapshots do §8 como unitários puros com o estimador de palavras, sem arquivos em `tests/snapshots/` (D-006).
- Resource `engramind://guides/{id}` (desenho §13, sem etapa prevista).
- `SettingView.choices` (só nas chaves `enum`).
- Linhas de `usage_log` pendentes limitadas a 50.000 entre lotes (`WARN usage.dropped`).

## Comandos de teste

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo nextest run --lib -E 'test(/context::|counters::|settings::/)'
cargo nextest run -E 'test(/^context::|logging::|matrix::|contract::/)'
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::/)'
cargo nextest run --locked   # suíte completa; ./scripts/test.sh usa llvm-cov, que não linka nesta máquina
```

## Rascunho de release notes

**0.5.0 — contexto (MVP do primeiro consumidor).** `get_context` entrega o bloco de memórias pronto para o prompt nas três vias: os canais `inject` do token, em ordem determinística, com texto completo até `context.budget_tokens`, títulos até `context.titles_budget_tokens` e o excedente contado. Formatos `tagged`, `markdown` e `json`. A `version` só muda com mudança real; `If-None-Match` responde `304` sem ler o banco, e o bloco fica num cache em memória. No MCP, `if_none_match`, os resources `engramind://context`, `engramind://channels/{slug}` e `engramind://guides/{id}` e o prompt `memory-writing`. A busca ganha `exclude_context` e `context_version`. `injected_count` e `usage_log` (por período) são gravados em lote. Novas configurações `context.*`, com orçamentos por canal.
