# 0020 — Troca de modelo e reindexação

**Data**: 2026-10-31 · **Etapa**: 007 (0.7.0) · **Status**: aceita

## Contexto

Salvar outro provedor ou modelo enfileira `reembed_all` com progresso em `get_status` (desenho §11), sem misturar dimensões. A primeira versão do plano terminava a reindexação quando `done >= total`, o que travava para sempre se uma memória fosse excluída no meio.

## Decisão

- **Validação antes, gravação junto.** `update_settings` compara as chaves de identidade (`provider`, `model`, `base_url`, `dimensions`, prefixos). Se mudaram, valida o provedor fora da transação — remoto: uma chamada com `"amostra"`; local: só confere o modelo (Q-018, sem baixar centenas de MB na requisição) — e grava a troca (`EmbeddingSwitch`) na mesma transação das configurações: `model_id` vigente, `embedding_state` e os lotes. Falha do provedor recusa a mudança inteira. Trocar a chave (`set_secret`), `batch_size` ou `timeout_ms` não reindexa.
- **O que reindexar.** Outro `model_id`: só as memórias sem vetor dele (inclui `off` → `on`); mesmo `model_id` com outra URL ou prefixo: todas; `reembed_all`: todas. `off`: `embedding_state = off`, sem jobs, vetores mantidos.
- **Lotes marcados.** Jobs `embed` com `{ memory_ids, reindex: started_at }` em lotes de `embedding.batch_size`; `schema_meta['embedding.reindex'] = { model_id, started_at, total }`.
- **Fim por jobs, não por contagem.** Termina quando não há job pendente nem falho da marca: apaga vetores de outros modelos, o registro e os modelos locais fora de uso (`models--*` em `MODELS_PATH`, Q-002). Se algum lote falhou de vez, `get_status.embedding.state = failed` e nada é apagado (Q-021); `reembed_all` recomeça. `409 REEMBED_IN_PROGRESS` só com jobs pendentes. `done` = vetores do modelo gravados desde `started_at`, limitado a `total`. No startup, `sync_embeddings` grava o `model_id` vigente e encerra a reindexação que terminou com o serviço parado.
- Durante a reindexação, a busca usa só o modelo vigente (D-008): o ramo vetorial cresce de zero até cobrir tudo e o FTS cobre o resto.

## Consequências

- Excluir memórias ou escrever novas no meio não trava nem engana o progresso.
- Uma instância por banco: o registro em `schema_meta` não coordena hosts.
