# Consolidação

O consolidador mantém a rede limpa: funde duplicatas, dá uma nota de importância para a ordem de corte do contexto e sinaliza (ou arquiva) as memórias sem uso. Ele roda por canal, agendado ou sob demanda, sempre pela fila (job `consolidate`), e nunca toca memórias `permanent` nem canais com `consolidation.enabled = false`. Vem desligado.

## Ligar

`consolidation.enabled` é global e pode ser sobreposto por canal:

```bash
engramind channels settings set eng/global consolidation.enabled true
engramind consolidation run --channel eng/global --dry-run --json
```

O horário vem de `consolidation.schedule` (cron de 5 campos, em UTC; padrão `0 3 * * *`). A cada minuto o agendador confere a expressão; no horário, enfileira um `consolidate` por canal habilitado que ainda não tenha um na fila. Se o serviço ficar desligado durante um ou mais horários, ele dispara uma vez só quando volta. Na primeira subida só registra o horário atual, sem disparar.

## Tarefas

As tarefas rodam sempre nesta ordem; `consolidation.tasks` escolhe quais (padrão: as três).

| Tarefa | O que faz | Precisa de |
|---|---|---|
| `duplicates` | Pares `active` do canal com cosseno ≥ `consolidation.duplicate_threshold` (0,92) vão ao Redator, que decide se funde e escreve o texto final. | Redator e embeddings |
| `importance` | O Redator dá uma nota de 0 a 1, em lotes de 20, às memórias `active` ainda não classificadas. | Redator |
| `stale` | Memória `active` sem edição, recuperação, injeção nem `mark_verified` há mais de `consolidation.stale_after_days` (90) vira `stale_candidate`; voltando a ser usada, deixa de ser. Com `consolidation.auto_archive_stale`, é arquivada. | — |

Sem Redator pronto (desligado ou sem provedor, modelo ou chave), `duplicates` e `importance` são puladas com `redator_not_configured`; com `embedding.provider = off`, `duplicates` é pulada com `embeddings_off`. As chamadas usam a configuração `redator.*` (modelo, `fallback_models`, `timeout_ms` e as `instructions` do canal) e ficam em `llm_calls` com `purpose = consolidation`.

Limites por canal e por execução: 20 pares e 200 memórias classificadas. Cada memória entra em no máximo um par por execução.

## Fusão

- Só entram memórias com vetor do modelo vigente (durante uma reindexação, as que ainda não têm vetor ficam de fora).
- O texto fundido precisa caber em `memory.text_min`/`memory.text_max`, não repetir outra memória do canal e passar na guarda de sentido contra **cada** original (`redator.min_similarity`); sem guarda disponível, o par não é fundido.
- A mais antiga (`created_at`, depois `id`) recebe o texto numa revisão `system/consolidador` (motivo `consolidation.merge`), mantém a categoria, soma `retrieval_count` e `injected_count` da outra e volta a ter o título e o vetor refeitos. A outra vira `archived` com `merged_into` apontando para a mantida.
- Um par recusado (pelo Redator ou pela guarda) não volta ao Redator enquanto os dois textos forem os mesmos.
- `revert_memory` e `restore_memory` desfazem tudo; `restore_memory` limpa `merged_into`.

## Importância e a `version` do contexto

A nota só afeta a ordem de corte do contexto (depois da prioridade, de `permanent` e da categoria). Para a ordem nunca mudar sem mudar a `version`, ela fica pendente e só passa a valer em `importance` na próxima mudança real do canal (qualquer escrita que incrementa a versão dele). Editar o texto zera a nota, e a memória volta a ser classificada na execução seguinte.

## Pré-visualizar e executar

`run_consolidation { channel?, tasks?, dry_run? }` (`POST /v1/consolidation/run`, token `admin` com `write`):

- `dry_run: true` responde `200` na hora, sem gravar e sem chamar o LLM, com o plano de cada canal: `duplicates` (`keep`, `archive`, `similarity`), `classify`, `stale` (`flag`, `clear`, `archive`) e `skipped`. A execução avalia exatamente esses candidatos; se o Redator funde ou não, só a execução sabe.
- Sem `dry_run`, responde `202` com um job por canal (`jobs: [{ channel, job_id, already_queued }]`); um canal que já tem um `consolidate` na fila devolve esse.
- `channel` ausente consolida todos os canais habilitados; `channel` explícito desligado responde `409 CONSOLIDATION_DISABLED` (o `dry_run` é permitido, para olhar antes de ligar).

```bash
engramind consolidation run --channel eng/global --tasks duplicates,stale --dry-run --json
engramind consolidation run
engramind jobs list --kind consolidate --json
```

## Acompanhar

- `get_status.consolidation`: canais habilitados, `schedule`, `next_run_at`, `last_scheduled_at`, jobs na fila e rodando, ações das últimas 24 h e as chamadas ao LLM.
- Auditoria: `consolidation.request` (pedido manual), `consolidation.run` por canal (contagens, sem textos), `memory.merge`, `memory.archive` (`reason: merged` ou `stale`).
- Métrica `engramind_consolidation_actions_total{task,action}`.
- Webhooks: a fusão gera `memory.updated` (a mantida) e `memory.archived` (a fundida); o arquivamento por desuso gera `memory.archived`. A nota de importância, a sinalização de desuso e `mark_verified` não geram evento, porque não mudam a `channel_version`.

## Conferência

`mark_verified` (`POST /v1/memories/{memory}/verify`, qualquer token com `write` efetivo no canal) registra que alguém conferiu a memória: grava `verified_at` e a auditoria `memory.verify` e limpa `stale_candidate`, sem nova revisão e sem mudar o contexto. Conferir é um sinal de uso para o desuso.

```bash
engramind memories verify m_8f2a1c3d
engramind memories list --stale --json
```
