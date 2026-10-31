# Implement - consolidação

**Data**: 2026-10-31

## Ordem real

1. Revisão do plano contra o código da 0.8.0 (colunas já existentes em `memories`, sem instante de injeção, runner sem renovação de lease, chamada LLM presa ao esquema do Redator) e Q-001 a Q-014, aprovadas pela usuária com as recomendações (`clarify.md`).
2. Spike T003: `croner` 4.0.2 com `CronParser::builder().seconds(Seconds::Disallowed).year(Year::Disallowed)` e `find_next_occurrence(&DateTime<Utc>, false)`; 5 campos obrigatórios (decisão 0025).
3. Migração `0009_consolidation.sql`; `promote_importance` em `bump_channel` e nas três atualizações de `channels.version` de `channels.rs`; `last_injected_at` no lote de injeções; edição, `revert` e texto do Redator zerando `importance_pending`; `restore_memory` limpando `merged_into`/`stale_candidate`.
4. `Memory.stale_candidate`/`merged_into`, filtro `stale`, `ChannelStats.stale`.
5. Tipos `SettingType::Cron` e `SettingType::ChoiceList`; seis chaves `consolidation.*`.
6. `Core::complete_with` extraído do Redator (prompt, esquema e validador por chamada).
7. `core::consolidation` (`plan` puro, `schedule`, `job`, prompts `merge.md`/`importance.md`), `Store` (`consolidation_input`, `enqueue_consolidation`, `merge_memories`, `decline_pair`, `set_importance_pending`, `apply_stale`, `finish_consolidation`, `consolidation_activity`, `mark_verified`), runner `Consolidate`, agendador em `background.rs`, `get_status.consolidation`, métrica.
8. Três vias (`run_consolidation` na tag `Consolidation`, `mark_verified` em `Memories`), `CONSOLIDATION_DISABLED`, guia `consolidacao`, guias e skill, `aicontext/consolidation.md`, desenho, decisões 0025–0026, versão 0.9.0.

## Mudanças além do contrato

- `ConsolidationRun` sempre traz `channels` e `jobs` (um deles vazio), para um só esquema nas duas respostas.
- `get_status.consolidation` traz `queued`/`running` no próprio bloco e `llm_last_24h` (o `LlmSummary` de `purpose = consolidation`); `last_24h` inclui `superseded`.
- Pedido manual grava a auditoria `consolidation.request` (entidade `consolidation`) além do `consolidation.run` de cada canal.
- Memórias de um par candidato ficam fora da classificação e do desuso na mesma execução (a fusão as altera), para o plano ser exatamente o que a execução faz.
- Sem guarda de sentido disponível, o par conta `failed` e não é gravado como recusado (volta na próxima execução); recusa pelo Redator, pela guarda ou por duplicata do texto fundido grava o par.
- O job confere de novo `consolidation.enabled` e o arquivamento do canal ao rodar (`done`/`skipped`).

## Comandos de teste

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::/)'
cargo nextest run -E 'test(/consolidation::|settings::|matrix::|contract::|webhooks::/)'
cargo nextest run --locked
```

## Rascunho de release notes

O Engramind se mantém limpo: com `consolidation.enabled` num canal, o consolidador agendado (ou `run_consolidation`) funde duplicatas pelo Redator com guarda de sentido, classifica a importância para a ordem de corte do contexto — sem nunca mudar a ordem sem mudar a `version` — e sinaliza (ou arquiva) memórias sem uso. O `dry_run` mostra o plano sem custo. `mark_verified` registra que uma memória foi conferida.
