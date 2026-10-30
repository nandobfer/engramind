# Contracts - consolidação

**Criado em**: 2026-10-30
**Status**: Draft
**Feature**: [`spec.md`](spec.md)

## Migração `0009_consolidation.sql`

```sql
ALTER TABLE memories ADD COLUMN importance_pending REAL NULL;
```

## Operações

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `run_consolidation` | admin·write | `POST /v1/consolidation/run` | `engramind consolidation run [--channel] [--tasks] [--dry-run]` |
| `mark_verified` | any·write | `POST /v1/memories/{memory}/verify` | `engramind memories verify <id>` |

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal |
|---|---|---|---|
| `consolidation.enabled` | booleano | false | sim |
| `consolidation.schedule` | cron | `0 3 * * *` | não |
| `consolidation.tasks` | lista (`duplicates`, `importance`, `stale`) | todas | não |
| `consolidation.duplicate_threshold` | número 0–1 | 0.92 | não |
| `consolidation.stale_after_days` | inteiro | 90 | não |
| `consolidation.auto_archive_stale` | booleano | false | não |
