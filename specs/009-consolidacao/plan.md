# Plan - consolidação

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Consolidador agendado e sob demanda, reaproveitando fila, Redator e embeddings.

## Áreas afetadas

**Core**: `src/core/consolidation/` (scheduler, duplicates, importance, stale, plan), `mark_verified`.
**Dados**: `migrations/0009_consolidation.sql`.
**Docs**: guia `operacao` (consolidação), `aicontext/consolidation.md`.

## Estratégia de implementação

1. Resolver Q-001 e Q-002.
2. Planejador puro (gera ações) usado por `dry_run` e pela execução.
3. Execução das ações com revisão, auditoria e eventos.
4. Promoção de `importance_pending` nas mutações.
5. Agendador, operações e versão 0.9.0.

## Estratégia de testes

**Integração**: `dry_run` = execução; `permanent` e canal desligado intocados; estabilidade da `version` com `importance_pending`; fusão com revisão e `merged_into`; desuso com e sem arquivamento automático.

## Riscos

| Risco | Mitigação |
|-------|-----------|
| Fusão errada do Redator | Revisão e `revert_memory` preservam tudo; `dry_run` para inspeção. |
