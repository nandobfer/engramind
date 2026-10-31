# Busca e dicas

## Onde vive

- `src/core/search.rs`: `search_memories` — ramo textual (`Store::search_text`), ramo vetorial (`Core::query_vector` + `Store::search_vectors`) e `fuse` (RRF ponderado, função pura com testes).
- `src/core/hints.rs`: `get_hints`.
- `src/rest/search.rs`, `get_hints`/`search_memories` em `src/mcp/tools.rs`, `engramind hints` e `engramind memories search` na CLI.
- SQL do ramo vetorial: `src/store/sqlite/vectors.rs` (`vec_distance_cosine`, varredura exata).

## Operações

- Qualquer token·read, tag `Search`: `search_memories` (`POST /v1/memories/search`) e `get_hints` (`POST /v1/hints`). As duas são leituras por POST (sem `Idempotency-Key`; exceção do teste de catálogo em `operations.rs`).

Guia: `busca-e-dicas`.

## Invariantes

- Canais: `Purpose::Search` (`inject`, `search`; `manage` e arquivados só citados). `get_hints` ainda descarta `Mode::Manage`, mesmo citado.
- O ramo vetorial compara só vetores do `model_id` vigente **e** da dimensão da consulta; nunca mistura modelos.
- `mode` descreve os ramos que rodaram: `hybrid` quando a consulta foi vetorizada (mesmo sem candidatos), `text` quando não. Falha, prazo (`embeddings::QUERY_TIMEOUT` = 2 s), `off` ou configuração incompleta nunca viram erro: `WARN embeddings.query_failed` e só o FTS.
- `score = Σ peso / (search.rrf_k + posição)`, 6 casas, desempate por `id`; cada ramo traz até o `limit` efetivo (`search.default_limit` quando ausente).
- `get_hints`: limiar `search.hint_threshold` sobre `1 - distância` (cosseno), exclui as memórias com texto completo no contexto (`context_for_search`), devolve só `{ id, title, channel }`; sem embeddings, lista vazia.
- As duas contam `retrieval_count` e gravam `usage_log` (`op` = nome da operação) pelos contadores em lote.
- `similar` na escrita continua textual (Jaccard, decisão 0012); o cosseno entre memórias é da 009.

Desenho: §9 e §11. Decisões: `0012`, `0018` e `0021`.
