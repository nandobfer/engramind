# Plan - memórias

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Implementar o ciclo de vida das memórias com as invariantes de escrita do §6, a busca textual e a skill.

## Áreas afetadas

**Core**: `src/core/memories/` (model, service, normalize, similar), `src/core/search.rs`, `src/core/counters.rs`, catálogo `SETTINGS`.
**Adaptadores**: `src/rest/memories.rs`, MCP, `engramind memories`, `engramind channels stats|settings`.
**Dados**: `migrations/0004_memories.sql`, `src/store/sqlite/memories.rs`, `jobs.rs` (só `enqueue`).
**Docs**: guia `escrever-memorias`, `skills/engramind/SKILL.md`, `aicontext/memories.md`.

## Estratégia de implementação

1. Resolver Q-001 a Q-004.
2. Migração com triggers do FTS e teste de sincronização.
3. Normalização e `content_hash`; título provisório.
4. `Store` de memórias com a transação única (memória + revisão + auditoria + versões + jobs + idempotência).
5. Serviço com acesso efetivo pelo resolvedor da 003.
6. Busca FTS com filtros e `similar`.
7. Configurações `categories`/`memory`, configurações por canal e `channel_stats`.
8. Contadores em lote.
9. Três vias, skill, guia, contrato e versão 0.4.0.

## Estratégia de testes

**Unit**: normalização, hash, título provisório, Jaccard.
**Integração**: cada mutação verificando revisão, auditoria, `memories.version`, `channels.version` e jobs na mesma transação (falha injetada no meio desfaz tudo); duplicata; conflito; `revert`; `move` com escopo nos dois canais; `delete` preservando a última revisão; FTS sem acentos; `manage` só explícito; contadores sem alterar `version`.
**Matriz**: operações de memória × modos × acesso da assinatura × escopos do token, nas três vias.
**Contrato**: skill contra `help --json`.

## Riscos

| Risco | Mitigação |
|-------|-----------|
| Trigger do FTS divergir em `UPDATE` parcial | Testes que conferem `memory_fts` após cada mutação; `rebuild` no `doctor`. |
| Backlog grande de jobs antes da 006 | Jobs idempotentes por `(kind, memory_id, version)`; a 006 descarta jobs de versões superadas. |
