# Consolidação: duplicatas, importância, desuso e `mark_verified`

## Onde vive

- `src/core/consolidation/mod.rs`: `Task`, `ConsolidationConfig` (`enabled` por canal), contrato de `run_consolidation` (`RunConsolidationBody`, `ConsolidationRun`, `ConsolidationPlan`), `Core::channel_plan`, `run_consolidation`, `consolidation_tick` (agendador) e `consolidation_status` (`get_status.consolidation`).
- `src/core/consolidation/plan.rs`: planejador **puro** (`plan`, `cosine_pairs`, `last_use`), limites `MAX_PAIRS` (20), `IMPORTANCE_BATCH` (20), `MAX_IMPORTANCE_CALLS` (10). O `dry_run` devolve o plano e o job age só sobre ele.
- `src/core/consolidation/job.rs`: job `consolidate` (`run_consolidate`; `Core::run_consolidate_jobs(now)` nos testes), prompts `merge.md`/`importance.md`, validação das respostas, `MemoryMerge`/`StaleWrite`; runner `Consolidate` em `src/core/jobs/runner.rs` (concorrência 1, lease fixo de 30 min).
- `src/core/consolidation/schedule.rs`: cron de 5 campos em UTC com `croner` (decisão 0025); `schema_meta['consolidation.last_scheduled']`.
- A chamada ao LLM é `Core::complete_with` do Redator (fallback, `llm_calls` com `purpose = consolidation`, métricas).
- `src/store/sqlite/consolidation.rs` (leitura do canal com os vetores do modelo vigente, fila, pares recusados, `importance_pending`, resumo); `src/store/sqlite/memories.rs` (`merge_memories`, `apply_stale`, `mark_verified`, `promote_importance`).
- Adaptadores: `src/rest/consolidation.rs` (tag `Consolidation`), `mark_verified` em `src/rest/memories.rs`, `src/mcp/tools.rs`, `engramind consolidation run`, `engramind memories verify`, `engramind memories list --stale`.

## Operações

| Operação | Tipo·escopo | Tag |
|---|---|---|
| `run_consolidation` | admin·write | `Consolidation` (guia `consolidacao`) |
| `mark_verified` | any·write, `write` efetivo no canal | `Memories` (guia `escrever-memorias`) |

## Invariantes

- Nunca toca `permanent`, canal arquivado ou com `consolidation.enabled = false` efetivo (o job confere de novo ao rodar).
- **A ordem do contexto nunca muda sem mudar a `version`**: a nota vai para `importance_pending` e só vira `importance` em `promote_importance`, chamada por toda transação que incrementa `channels.version` (`bump_channel` e as três atualizações de `channels.rs`). Escrita nova que incrementa `channels.version` sem passar por `bump_channel` precisa chamar `promote_importance`. Edição de texto, `revert`, texto do Redator e fusão zeram `importance` e `importance_pending`.
- Ações são transações curtas com compare-and-set no `content_hash` lido no plano; memória mudada → `superseded`. O LLM nunca roda dentro de transação; falha do LLM conta `failed` e o job termina `done`.
- Fusão: guarda de sentido contra cada original (sem guarda, não funde); a mais antiga recebe texto, contadores e `redact`/`embed`; a outra fica `archived` com `merged_into`; eventos `memory.updated` + `memory.archived`. Recusa grava `consolidation_pairs` (vale enquanto os dois hashes forem os mesmos).
- Desuso: último uso = maior entre `updated_at`, `last_retrieved_at`, `last_injected_at` (lote de injeções) e `verified_at`. Sinalizar/limpar não muda `version` nem gera evento; arquivar sim.
- `mark_verified`, importância e sinalização de desuso não geram evento (`WITHOUT_EVENTS` em `tests/it/webhooks.rs`) e não chamam `network_changed()`; fusão e arquivamento chamam.
- Auditoria e resumos nunca têm texto nem título: `consolidation.request`, `consolidation.run`, `memory.merge`, `memory.archive` (`reason`), `memory.verify`.

Guias: `consolidacao`, `escrever-memorias`, `operacao`, `integracao-gestor`. Desenho: §6, §8, §10, §12, §17. Decisões: `0025`, `0026`.
