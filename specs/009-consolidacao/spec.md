# Spec - consolidação

**Criado em**: 2026-10-30
**Atualizado em**: 2026-10-31 (planejamento revisado sobre a 0.8.0)
**Status**: Implementado (0.9.0)
**Versão alvo**: 0.9.0
**Input**: Desenho §2 (Dreamer, contagem de recuperação, `verify`), §6 (`memories`), §7 (edição zera `importance`, status), §8 (ordem e estabilidade), §10, §12 (`consolidation`), §13 (`run_consolidation`), §22 fase 4 (`mark_verified`); decisão 0021 (cosseno entre memórias fica para a 009).

## Objetivo

A rede se mantém limpa sozinha (agendada) ou sob demanda: duplicatas fundidas pelo Redator, importância classificada para a ordem de corte do contexto, memórias em desuso sinalizadas (e, se configurado, arquivadas) e conferências registradas com `mark_verified` — sem quebrar a estabilidade da `version` do contexto.

## Estado de partida (0.8.0)

- `memories` já tem `importance`, `retrieval_count`, `injected_count`, `last_retrieved_at`, `verified_at`, `merged_into` e `stale_candidate` (migração `0004`); nenhum código escreve `importance`, `verified_at`, `merged_into` nem `stale_candidate`.
- A ordem do contexto (`src/core/context/order.rs`) já usa `importance` quando classificada; a edição do texto já zera `importance`.
- `Memory` (resposta pública) expõe `importance` e `verified_at`, mas não `stale_candidate` nem `merged_into`.
- `injected_count` é somado em lote (`counters.rs`), sem instante: não existe "última injeção".
- Não há vetor entre memórias em uso: `similar` é Jaccard (0012); o ramo vetorial só compara consulta × memória.
- A fila tem runners `redact`, `embed` e `webhook`, sem renovação de lease; tarefas periódicas vivem em `src/core/background.rs`; não há agendamento por cron nem chaves `consolidation.*` no catálogo.
- O cliente LLM, o prompt com marcas aleatórias, `llm_calls` (com `purpose`) e as métricas por finalidade já existem.

## Escopo

- Chaves `consolidation.*` (com `enabled` por canal) e os tipos de configuração novos (cron, lista de opções).
- Agendador por expressão cron sobre a fila: um job `consolidate` por canal elegível no horário.
- Tarefas por canal, nesta ordem: `duplicates`, `importance`, `stale`.
- `run_consolidation { channel?, tasks?, dry_run? }` (admin·write).
- `mark_verified { id }` (any·write).
- `stale_candidate` e `merged_into` na resposta das memórias; filtro `stale` em `list_memories`; contagem em `channel_stats`.
- `get_status.consolidation`, métrica de ações, guia `consolidacao` (admin), skill e documentação.

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | Nunca altera memórias `permanent` (nem as usa como par) nem canais com `consolidation.enabled = false` efetivo (sobreponível por canal); padrão global desligado. Canal arquivado nunca é consolidado. |
| FR-002 | Duplicatas: pares `active` do mesmo canal, com vetor do modelo vigente e cosseno ≥ `consolidation.duplicate_threshold`, vão ao Redator, que decide se funde e devolve o texto final (Q-006). Fusão aceita: a mais antiga (`created_at`, `id`) recebe o texto numa revisão `system/consolidador`; a outra vira `archived` com `merged_into`; tudo numa transação, com auditoria e eventos (Q-009). Par recusado não volta ao Redator enquanto os dois textos forem os mesmos (Q-014). |
| FR-003 | Importância: o Redator dá nota de 0 a 1 em lote às memórias `active` ainda não classificadas (Q-007); a nota vai para `importance_pending` e só passa a valer (`importance`) na próxima transação que incrementa `channels.version` do canal (D-001). Editar o texto zera as duas. |
| FR-004 | Desuso: memória `active` cujo último sinal de uso (Q-003) é mais antigo que `consolidation.stale_after_days` → `stale_candidate = true`; sinal novo, edição ou `mark_verified` limpa. Com `consolidation.auto_archive_stale = true`, as candidatas são arquivadas (revisão `system/consolidador`, evento `memory.archived`). Sinalizar não muda `version` nem o contexto (Q-002). |
| FR-005 | `run_consolidation` com `dry_run = true` devolve, sem gravar e sem chamar o LLM, o plano por canal: pares candidatos, memórias a classificar, sinalizações, limpezas, arquivamentos e tarefas puladas com o motivo (Q-004). Sem `dry_run`, enfileira um job `consolidate` por canal e devolve os ids (`202`). |
| FR-006 | `mark_verified { id }` grava `verified_at` e auditoria `memory.verify`, limpa `stale_candidate`, sem nova revisão, sem mudar `memories.version`/`channels.version` e sem evento (Q-001, Q-009). Exige `write` efetivo no canal. |
| FR-007 | Chamadas do consolidador gravam `llm_calls` com `purpose = consolidation`, usando a configuração `redator.*` (provedor, modelo, chave, `fallback_models`, timeout e `instructions` do canal) (Q-012). |
| FR-008 | Agendador: avalia `consolidation.schedule` (cron de 5 campos, UTC) a cada minuto; no horário (ou uma única vez depois de um período desligado) enfileira um `consolidate` por canal elegível que não tenha um na fila; o último disparo sobrevive a reinício (Q-010). |
| FR-009 | Execução: cada ação é uma transação própria com compare-and-set (`content_hash` lido no plano); ação sobre memória alterada depois do plano é pulada. Ao terminar, grava a auditoria `consolidation.run` do canal com as contagens e o resumo em `get_status.consolidation` (Q-005). |
| FR-010 | Sem Redator pronto, `duplicates` e `importance` são puladas com motivo; com `embedding.provider = off` ou sem vetores do modelo vigente, `duplicates` é pulada; `stale` nunca depende de provedor (Q-012). |
| FR-011 | `Memory` ganha `stale_candidate` e `merged_into`; `list_memories` aceita `stale=true|false`; `channel_stats` conta as candidatas; `restore_memory` limpa `merged_into` e `stale_candidate` (Q-002). |
| FR-012 | `get_status.consolidation`: canais habilitados, `schedule`, próximo e último disparo, jobs na fila/rodando e as ações e chamadas do LLM nas últimas 24 h. Métrica `engramind_consolidation_actions_total{task,action}`. |
| FR-013 | Guias: `consolidacao` (novo, admin), `escrever-memorias` (`mark_verified`, `stale_candidate`, `merged_into`), `configuracoes`, `erros`, `operacao` e `cli`; skill. |

## Fora do escopo

- Verificação da memória contra código (é do consumidor; o serviço só registra com `mark_verified`).
- Fusão entre canais diferentes, fusão de mais de duas memórias por vez e reclassificação periódica de memórias já classificadas (Q-007).
- Trocar o `similar` da escrita para cosseno (decisão 0021 mantida: a escrita não espera o provedor).
- Evento de webhook para importância, desuso e `mark_verified` (Q-009).
- Escala (`vec0` particionado, Postgres).

## Critérios de sucesso

- Com LLM roteirizado e `HashEmbedder`, uma execução funde um par duplicado (revisão, `merged_into`, eventos), classifica as não classificadas e sinaliza o desuso; o `dry_run` anterior lista exatamente os mesmos pares, as mesmas memórias a classificar e as mesmas sinalizações.
- `permanent`, canal desligado e canal arquivado ficam intocados; par recusado não volta ao Redator na execução seguinte.
- A `version` do contexto não muda pela classificação; muda (com a nova ordem) só na próxima mutação real do canal, inclusive depois de reiniciar o serviço.
- `mark_verified` não muda `version` nem o bloco do contexto e limpa `stale_candidate`.
- O agendador dispara uma vez por horário, não duplica jobs na fila e retoma depois de reiniciar.
