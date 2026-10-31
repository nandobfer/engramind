# Plan - consolidação

**Data**: 2026-10-31
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Consolidador agendado (cron) e sob demanda (`run_consolidation`), um job `consolidate` por canal com as tarefas `duplicates`, `importance` e `stale`, reaproveitando a fila, o cliente LLM do Redator, os vetores e os eventos; `importance_pending` para a ordem só mudar com a `version`; `mark_verified`.

## Contexto técnico

- Colunas de consolidação já existem em `memories` (`0004`); faltam `importance_pending`, `last_injected_at` e a tabela de pares recusados (`0009`).
- Toda mudança de `channels.version` passa por `bump_channel` (`src/store/sqlite/memories.rs`) ou por três `UPDATE` em `src/store/sqlite/channels.rs`: é ali que a promoção entra (D-001/Q-008); convém unificar as quatro num helper do `journal`/`channels` para não esquecer nenhuma.
- O cliente LLM (`Core::complete` no Redator) está acoplado ao esquema do Redator (`SCHEMA_NAME`, `validate`); a consolidação precisa de uma variante genérica (sistema, usuário, esquema, validador) que preserve fallback, `llm_calls` e métricas.
- A guarda de sentido (`MeaningGuard`) compara um par original × revisado; a fusão chama duas vezes (ou uma com três textos) e falha fechada.
- O runner não renova lease: lease fixo de 30 min e ações idempotentes por compare-and-set (Q-005).
- `background.rs` já roda tarefas periódicas; o agendador entra como mais um `interval` de 60 s.
- Dependência nova: `croner` 4 (decisão 0025), verificada no spike com a documentação atual (Context7).

## Áreas afetadas

**Core**: `src/core/consolidation/` (novo: `mod.rs` com config e `run_consolidation`, `plan.rs`, `job.rs`, `schedule.rs`, `merge.rs`, `importance.rs`, `stale.rs`, `merge.md`, `importance.md`), `src/core/memories.rs` (`mark_verified`, filtro `stale`, `restore` limpando `merged_into`), `src/core/redator/mod.rs` (extrair a chamada LLM genérica), `src/core/redator/guard.rs`, `src/core/jobs/runner.rs` (runner `consolidate`), `src/core/background.rs`, `src/core/counters.rs`, `src/core/settings.rs` (tipos `Cron` e `ChoiceList`, chaves), `src/core/status.rs`, `src/core/operations.rs` (tag `Consolidation`), `src/core/model.rs` (`Memory.stale_candidate`, `merged_into`, tipos do plano), `src/core/channels.rs` (`channel_stats.stale`), `src/error.rs` (`CONSOLIDATION_DISABLED`), `src/metrics.rs`.
**Adaptadores**: `src/rest/consolidation.rs` (novo), `src/rest/memories.rs`, `src/app.rs`, `src/openapi.rs`; `src/mcp/tools.rs`; `src/cli/{mod,remote,help}.rs`.
**Dados**: `migrations/0009_consolidation.sql`; `src/store/mod.rs`; `src/store/sqlite/{memories,channels,context,consolidation (novo),mod}.rs`.
**Docs**: `docs/guides/consolidacao.md` (novo, admin), `escrever-memorias.md`, `configuracoes.md`, `erros.md`, `operacao.md`, `integracao-gestor.md` (eventos do consolidador), `cli.md`, `overview.md`; `skills/engramind/SKILL.md`; `aicontext/consolidation.md` (novo), `aicontext/{README,memories,context,jobs,settings}.md`; `docs/design.md` §6/§10/§12/§13/§17/§20; `docs/decisions/0025-agendador-cron.md`, `0026-consolidacao.md`; `CHANGELOG.md`.

## Estratégia de implementação

1. **T001** resolver Q-001 a Q-014; ajustar spec, contracts e desenho.
2. **Spike T003 (portão)**: `croner` 4 — parse de 5 campos, `next_after` em UTC, erro de expressão inválida; registrar a decisão 0025 com a versão exata.
3. Migração `0009`; promoção de `importance_pending` em toda transação de `channels.version`; `last_injected_at` em `bump_injections`; zerar pendência na edição/`revert`. Testes da estabilidade antes de qualquer classificação existir (inserção direta pelo `Store` de teste).
4. Configurações: `SettingType::Cron` e `SettingType::ChoiceList`, as seis chaves, guia `configuracoes`.
5. `mark_verified` (core → Store → três vias) e os campos `stale_candidate`/`merged_into`, filtro `stale`, `channel_stats.stale`, `restore`.
6. Planejador puro (`plan`, `cosine_pairs`, desuso) com testes unitários; `Store::consolidation_input`.
7. `run_consolidation` com `dry_run` (síncrono) e enfileiramento (`already_queued`, `CONSOLIDATION_DISABLED`).
8. Chamada LLM genérica extraída do Redator; job `consolidate` com as três tarefas, CAS, auditoria `consolidation.run`, eventos, métricas e `schema_meta['consolidation.last_run']`.
9. Agendador em `background.rs` (`consolidation_tick`).
10. `get_status.consolidation`; métrica.
11. Três vias, OpenAPI, guias, skill, contrato e matriz; `aicontext`, desenho, decisões, versão 0.9.0.

## Estratégia de testes

**Unitários**
- `cosine_pairs`: limiar, ordem por cosseno e desempate, pareamento guloso (cada memória uma vez), limite de 20.
- Plano: `permanent` fora, sem vetor fora, par recusado com hash igual ignorado e com hash diferente reconsiderado, lote de importância (20 × 10), desuso nos dois sentidos com cada sinal (`updated_at`, `last_retrieved_at`, `last_injected_at`, `verified_at`), `auto_archive_stale`.
- Validação das respostas do LLM (fusão sem texto, texto fora dos limites, notas fora de 0–1, id faltando/repetido/estranho).
- `next_after` e o disparo único depois de horários perdidos; primeiro start sem disparo.
- Tipos `Cron` e `ChoiceList` (válidos e inválidos).

**Integração (`tests/it/consolidation.rs`, novo módulo do binário `tests/it/`)**
- Execução completa com `ScriptedLlm` e `HashEmbedder`: fusão (texto na mais antiga, revisão `system/consolidador`, `merged_into`, soma dos contadores, `redact`/`embed` reenfileirados, `memory.updated` + `memory.archived`), recusa gravada e não repetida, classificação em `importance_pending`, desuso sinalizado e limpo, arquivamento automático com evento.
- `dry_run` antes da execução lista exatamente os candidatos que a execução avalia (comparando com a auditoria `consolidation.run`).
- Estabilidade: `get_context` com a mesma `version` e o mesmo bloco depois da classificação, inclusive com um `Core` novo sobre o mesmo banco; a próxima `create_memory` no canal muda a `version` e aplica a ordem.
- `permanent`, canal desligado (global e sobreposto por canal), canal arquivado intocados; `CONSOLIDATION_DISABLED` sem `dry_run`; `already_queued`.
- Guarda de sentido recusando a fusão; embeddings `off` e Redator desligado pulando as tarefas; falha do LLM contada sem falhar o job; memória editada entre o plano e a ação → `superseded`.
- `mark_verified`: `verified_at`, auditoria, sem mudar `version`/contexto, limpa `stale_candidate`, `404` fora do alcance, `403` sem `write`.
- Agendador com relógio injetado: dispara no horário, não duplica com job na fila, retoma após reinício.
- `get_status.consolidation` e `llm_calls` com `purpose = consolidation`.

**Contrato/matriz**
- `run_consolidation` e `mark_verified` em `OPERATIONS`, OpenAPI, MCP, CLI e guias; tag `Consolidation` em `ADMIN_TAGS` coberta por `consolidacao`; chaves e `CONSOLIDATION_DISABLED` documentados; `WITHOUT_EVENTS` atualizado.
- `tests/it/matrix.rs`: casos para as duas operações (inclusive assinatura só leitura em `mark_verified`).

## Observabilidade

- `consolidation.scheduled` (`INFO`, canais enfileirados), `consolidation.completed` (`INFO`, canal e contagens), `consolidation.task_skipped` (`DEBUG` quando o motivo é configuração, `WARN` quando é falha do provedor), `consolidation.llm_failed` (`WARN`), `consolidation.superseded` (`DEBUG`). Nunca texto nem título.

## Riscos

| Risco | Mitigação |
|-------|-----------|
| Fusão errada do Redator | Guarda de sentido contra os dois originais, revisão `system/consolidador`, `revert_memory`/`restore_memory` preservam tudo; `dry_run` para inspeção; desligado por padrão. |
| Ordem do contexto mudar sem `version` | `importance_pending` + promoção em toda transação de `channels.version` (helper único) + teste com `Core` novo sobre o mesmo banco. |
| Custo do LLM descontrolado | Limites constantes por canal e por execução (20 pares, 200 classificações), pares recusados memorizados, sem reclassificação periódica, `llm_calls`/métricas por finalidade. |
| Job longo perder o lease | Lease de 30 min, concorrência 1, ações por CAS: reexecução não duplica efeito. |
| O(n²) do cosseno em canal grande | Varredura em Rust só no horário, sobre `active` com vetor; medição no teste com 5.000 vetores; se não couber, a escala (`vec0`) ganha spec. |
| Escritor único segurando transações longas | Uma transação curta por ação; LLM sempre fora da transação. |
| Agendador disparar em dobro | `schema_meta['consolidation.last_scheduled']` + verificação de job `queued`/`running` do canal na mesma transação do enfileiramento. |

## Versão e entrega

- `0.9.0` em `Cargo.toml`/`Cargo.lock`; `CHANGELOG.md` com a etapa 009.
- `docs/openapi.json` regenerado com `UPDATE_SNAPSHOTS=1`.
- `implement.md` e `report.md` ao final; deploy pela `main` e verificação com `get_status.consolidation` em produção (canal de teste com `consolidation.enabled = true` e `dry_run`).
