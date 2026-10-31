# Report - consolidação

**Data**: 2026-10-31 · **Versão**: 0.9.0

## Entrega

FR-001 a FR-013 implementados (detalhes em `implement.md`). Q-001 a Q-014 resolvidas (`clarify.md`); decisões 0025 e 0026.

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run --locked` | 314 testes, todos passam |
| Execução completa (critério de sucesso) | Com LLM roteirizado e `HashEmbedder`: o `dry_run` lista o par (com o cosseno), a memória a classificar e a sinalização, sem chamar o LLM nem enfileirar; a execução funde o par (texto na mais antiga, revisão `system/consolidador`, `merged_into`, `title_state = pending`), grava a nota em `importance_pending`, sinaliza o desuso, registra `consolidation.run` e 2 `llm_calls` com `purpose = consolidation`; webhooks `memory.updated` + `memory.archived`; `get_status.consolidation`, `list_memories?stale=true` e `channel_stats.stale` |
| Estabilidade da `version` (critério de sucesso) | Depois da classificação, mesma `version` e mesmo bloco, inclusive num `Core` novo sobre o mesmo banco; a próxima `create_memory` muda a `version` e aplica a ordem nova; editar zera a nota |
| Intocáveis (critério de sucesso) | Par recusado não volta ao Redator; `permanent` fora dos pares e da classificação; canal desligado → `409 CONSOLIDATION_DISABLED` (o `dry_run` passa); sem canal só os habilitados; `already_queued`; canal arquivado → `409 CHANNEL_ARCHIVED`; `tasks` inválidas → `400` |
| Provedores | Guarda recusando → `declined`; LLM `500` → `failed` com o job `done`; `embedding.provider = off` → `embeddings_off`; Redator desligado → `redator_not_configured` |
| Desuso e `mark_verified` (critério de sucesso) | Arquivamento automático com revisão `consolidation.stale`, `channels.version + 1` e auditoria; `mark_verified` grava `verified_at`, limpa `stale_candidate`, não muda `version`, `updated_at`, revisões nem o bloco do contexto; `404` fora do alcance |
| Agendador (critério de sucesso) | Primeira subida só registra; dispara às 03:00 UTC uma vez; vários horários perdidos → um disparo; não duplica com job pendente; canal desligado depois do enfileiramento → job sem efeito |
| CAS | Fusão com hash diferente → `Conflict("MEMORY_CHANGED")`; pares recusados com hash velho somem no fim da execução |
| Matriz e contrato | 67 operações nas três vias; OpenAPI regenerado; tag `Consolidation` coberta pelo guia `consolidacao`; chaves, `CONSOLIDATION_DISABLED` e comandos documentados; `WITHOUT_EVENTS` com `run_consolidation` e `mark_verified` |

## Pendências

- Verificação em produção: ligar `consolidation.enabled` num canal de teste e conferir o `dry_run` e `get_status.consolidation` depois do deploy.
- O custo do cosseno O(n²) por canal não foi medido com 5.000 vetores (plano); fica para quando um canal real chegar perto disso.
