# Spec - consolidação

**Criado em**: 2026-10-30
**Status**: Draft
**Versão alvo**: 0.9.0
**Input**: Desenho §2 (Dreamer), §8 (estabilidade), §10, §12 (`consolidation`), §22 fase 4 (`mark_verified`).

## Objetivo

A rede se mantém limpa sozinha ou sob demanda: duplicatas fundidas, importância classificada para a ordem de corte, memórias em desuso sinalizadas e conferências registradas.

## Escopo

- Agendador por expressão cron (`consolidation.schedule`) sobre a fila.
- Tarefas `duplicates`, `importance` e `stale`, por canal.
- `run_consolidation { channel?, tasks?, dry_run? }`.
- `mark_verified`.
- Configurações `consolidation.*`.

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | Nunca altera memórias `permanent` nem canais com `consolidation.enabled = false` (sobreponível por canal); padrão global desligado. |
| FR-002 | Duplicatas: pares com cosseno ≥ `consolidation.duplicate_threshold` vão ao Redator, que decide fundir e o texto final; a mais antiga recebe o texto (revisão `system/consolidador`) e a outra vira `archived` com `merged_into`; auditoria e eventos. |
| FR-003 | Importância: o Redator dá nota de 0 a 1 em lote; a nota só afeta a ordem de corte do contexto e só passa a valer na próxima mudança real do canal (D-001). |
| FR-004 | Desuso: sem recuperação e sem injeção há mais de `consolidation.stale_after_days` → `stale_candidate = true`; arquivamento automático só com `consolidation.auto_archive_stale`. |
| FR-005 | `run_consolidation` com `dry_run` devolve as ações previstas sem gravar; sem `dry_run`, enfileira e devolve o id do job. |
| FR-006 | `mark_verified { id }` grava `verified_at` e auditoria, sem nova revisão e sem mudar `version` nem o contexto. |
| FR-007 | Chamadas ao Redator gravam `llm_calls` com `purpose = consolidation`. |

## Fora do escopo

- Verificação da memória contra código (é do consumidor).

## Critérios de sucesso

- Com LLM roteirizado e `HashEmbedder`, uma execução funde um par duplicado, classifica importâncias e sinaliza desuso; o `dry_run` prevê exatamente as mesmas ações.
- `version` do contexto não muda por classificação até uma mudança real.
