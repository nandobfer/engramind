# Tasks - consolidação

**Input**: `spec.md`, `clarify.md`, `contracts.md`, `plan.md`

## Planejamento

- [x] T001 - Resolver Q-001 a Q-014 com a usuária; ajustar spec, contracts e desenho conforme as respostas.

## Portão técnico

- [x] T003 - Spike: `croner` 4 (expressão de 5 campos, `next_after` em UTC, erro de expressão inválida); registrar a decisão 0025 com a versão exata.

## Dados

- [x] T002 - Migração `0009_consolidation.sql` (`importance_pending`, `last_injected_at`, `consolidation_pairs`); teste de migração.
- [x] T004 - Helper único de `channels.version` com a promoção de `importance_pending`; `last_injected_at` em `bump_injections`; edição/`revert` zerando a pendência e `stale_candidate`.
- [x] T005 - `Store`: `consolidation_input`, `merge_memories`, `decline_pair`, `set_importance_pending`, `apply_stale`, `mark_verified`, contagens de 24 h.

## Configurações e memórias

- [x] T010 - `SettingType::Cron` e `SettingType::ChoiceList`; as seis chaves `consolidation.*`.
- [x] T011 - `mark_verified`; `Memory.stale_candidate`/`merged_into`; filtro `stale`; `channel_stats.stale`; `restore_memory` limpando `merged_into`.

## Consolidador

- [x] T012 - Planejador puro (`plan`, `cosine_pairs`, desuso, lotes, pares recusados).
- [x] T013 - Chamada LLM genérica extraída do Redator (fallback, `llm_calls`, métricas); prompts `merge.md` e `importance.md`.
- [x] T014 - Job `consolidate`: duplicatas (guarda, CAS, eventos), importância (`importance_pending`), desuso (flag/clear/archive), auditoria `consolidation.run`, `schema_meta['consolidation.last_run']`, métrica.
- [x] T015 - `run_consolidation` (`dry_run` síncrono, enfileiramento, `already_queued`, `CONSOLIDATION_DISABLED`).
- [x] T016 - Agendador (`consolidation_tick` em `background.rs`).
- [x] T017 - `get_status.consolidation`.

## Três vias e contrato

- [x] T020 - `OPERATIONS` (tag `Consolidation`), REST com `#[utoipa::path]`, MCP, CLI (`consolidation run`, `memories verify`, `memories list --stale`).
- [x] T021 - `docs/openapi.json` (`UPDATE_SNAPSHOTS=1`), `ADMIN_TAGS`, `catalog.rs` (guia `consolidacao`), `WITHOUT_EVENTS`.

## Testes

- [x] T030 - Unitários: `cosine_pairs`, plano, validação das respostas, agendador, tipos de configuração.
- [x] T031 - Integração: execução completa, `dry_run` = candidatos, estabilidade da `version` (com `Core` novo), intocáveis, provedores indisponíveis, `superseded`, agendador.
- [x] T032 - `mark_verified` (efeitos, `404`, `403`).
- [x] T033 - Matriz e contrato.

## Operação

- [x] T040 - Guias (`consolidacao`, `escrever-memorias`, `configuracoes`, `erros`, `operacao`, `integracao-gestor`, `cli`, `overview`) e skill.
- [x] T041 - `aicontext/consolidation.md`, `aicontext/{README,memories,context,jobs,settings}.md`; `docs/design.md` §6/§10/§12/§13/§17/§20; decisões 0025–0026.
- [x] T042 - Versão `0.9.0`, `Cargo.lock`, `CHANGELOG.md`.
- [x] T043 - `implement.md` e `report.md`; verificação final (`fmt`, `clippy`, testes afetados, snapshot); verificação em produção.
