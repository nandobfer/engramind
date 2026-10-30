# Plan - contexto

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Renderizador determinístico com cache e ETag, exposto nas três vias e como resource MCP.

## Áreas afetadas

**Core**: `src/core/context/` (select, order, budget, render/{tagged,markdown,json}, version, cache), `src/core/usage.rs`.
**Adaptadores**: `src/rest/context.rs` (ETag/304), MCP (ferramenta, resources, prompt), `engramind context`.
**Dados**: `migrations/0005_usage.sql`, leitura otimizada de memórias por canais.
**Docs**: guias `contexto-e-cache` e `integracao-consumidor` (versão inicial), `aicontext/context.md`.

## Estratégia de implementação

1. Resolver Q-001 a Q-003.
2. Estimador de tokens (`tiktoken-rs`) atrás de `TokenEstimator`.
3. Seleção e ordenação puras (sem I/O) com testes de propriedade da ordem.
4. Faixas e renderizadores; snapshot do §8.
5. `version`, cache e `304`.
6. `injected_count` e `usage_log` em lote; `exclude_context` na busca.
7. Três vias, resources e prompt; guias; versão 0.5.0.

## Estratégia de testes

**Unit**: ordenação (cada critério isolado), cortes de faixa, escape (D-002), `version`.
**Integração**: `304`, invalidação por cada tipo de mudança, estabilidade após contadores, admin sem filtro, `manage`/`search` fora do bloco.
**Snapshot**: `tests/snapshots/context_tagged.txt`, `context_markdown.txt`, `context_json.json`.

## Riscos

| Risco | Mitigação |
|-------|-----------|
| `tiktoken-rs` lento na inicialização | Carregar o BPE uma vez no startup (lazy estático). |
| Cache servir bloco de assinatura revogada | Chave inclui o conjunto alcançado, recalculado pelo resolvedor (que já invalida por mudança). |
