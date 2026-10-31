# Plan - contexto

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Renderizador determinístico com cache e ETag, exposto nas três vias e como resource MCP.

## Áreas afetadas

**Core**: `src/core/context/` (`order`, `budget`, `render`, `version`, `cache`, `estimator`, `mod` com `get_context`, `context_for_search` e `channel_titles`), `src/core/counters.rs` (`injected_count` e `usage_log`, D-005), `src/core/search.rs` (`exclude_context`, `context_version`), `src/core/settings.rs` (tipo `enum`, chaves `context.*`).
**Adaptadores**: `src/rest/context.rs` (ETag/304), `src/mcp/tools.rs` (ferramenta e capacidades), `src/mcp/resources.rs` (resources e prompt), `engramind context` e `memories search --exclude-context`.
**Dados**: `migrations/0005_usage.sql`; `src/store/sqlite/context.rs` (leitura num instantâneo, lotes).
**Docs**: guias `contexto-e-cache` e `integracao-consumidor` (primeira versão), `integracao-mcp`, `configuracoes`, `cli`, `escrever-memorias`, `overview`, `erros`, skill, `docs/prompts/memory-writing.md`, `aicontext/context.md`, decisões `0013` e `0014`.

## Estratégia de implementação

1. Resolver Q-001 a Q-023 (`clarify.md`).
2. Estimador de tokens (`tiktoken-rs`) atrás de `TokenEstimator`, carregado no primeiro uso.
3. Ordenação e faixas puras (sem I/O), com unitários de cada critério e de cada corte.
4. Renderizadores e escape; snapshot do §8 como unitário puro (D-006).
5. `version`, cache e `304` a partir da rede em cache.
6. `injected_count` e `usage_log` em lote; `exclude_context` na busca.
7. Três vias, resources e prompt; guias; versão 0.5.0.

## Estratégia de testes

**Unit**: ordenação (cada critério isolado), cortes de faixa e teto por canal, escape (D-002), `version` e `If-None-Match`, LRU, configurações `enum`, snapshots `tagged`/`markdown`/`json` do §8.
**Integração** (`tests/it/context.rs`): `304` nas três vias, invalidação por cada tipo de mudança, estabilidade após contadores, admin sem filtro, `manage`/`search` fora do bloco, cortes, formatos, `usage_log` por período, `exclude_context`, resources e prompt, CLI.
**Contrato e logs**: matriz com `get_context`, argumento `if_none_match`, resources/prompt documentados, `304` em `DEBUG` e o texto fora dos logs.

## Riscos

| Risco | Mitigação |
|-------|-----------|
| `tiktoken-rs` lento na inicialização | Singleton no primeiro uso, aquecido pelo `serve` em `spawn_blocking`; renderização também em `spawn_blocking`. |
| Cache servir bloco de assinatura revogada | Chave = digest sobre o conjunto alcançado, recalculado pelo resolvedor (que já invalida por mudança). |
| Título trocado sem mudar a `version` | 006 FR-007 ajustado (D-008). |
