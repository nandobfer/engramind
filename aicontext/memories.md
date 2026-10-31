# Memórias

## Onde vive

- `src/core/memories.rs`: entradas, validação (texto, categoria, origem, status), `reach_memory`, `similar` e as operações de memória.
- `src/core/memory_text.rs`: funções puras — `content_hash`, título provisório, termos sem acento e Jaccard.
- Busca e dicas: [`search.md`](search.md).
- `src/core/counters.rs`: `retrieval_count`/`last_retrieved_at` em lote (flush a cada 5 s em `background.rs` e no shutdown).
- `src/core/channels.rs` (`channel_stats`) e `src/core/settings.rs` (`get_channel_settings`, `update_channel_settings`, `categories.list`, `memory.*`).
- `src/store/sqlite/memories.rs`: SQL, FTS5, a transação única de cada mutação e a fila `jobs` (só `enqueue`).
- Adaptadores: `src/rest/memories.rs`, `src/mcp/tools.rs`, `engramind memories …` em `src/cli/`.
- Migração `migrations/0004_memories.sql`.

## Operações

- Qualquer token·read: `list_memories`, `get_memory`, `memory_history` (tag `Memories`); `channel_stats` (tag `Channels`). `search_memories` está na tag `Search` (0.7.0).
- Qualquer token·write: `create_memory`, `update_memory`, `archive_memory`, `restore_memory`, `pin_memory`, `unpin_memory`, `move_memory`, `revert_memory` e `mark_verified` (0.9.0, sem revisão nem `version`; ver [`consolidation.md`](consolidation.md)).
- Qualquer token·delete: `delete_memory`.
- admin: `get_channel_settings` (read), `update_channel_settings` (write), tag `Settings`.

Guia: `escrever-memorias`; skill: `skills/engramind/SKILL.md`.

## Invariantes

- O escopo da operação é conferido pelo catálogo; o escopo no canal, por `require_channel`. Memória num canal fora do alcance ou da seleção → `404 MEMORY_NOT_FOUND` (nunca `403`).
- Toda mutação grava memória, revisão (menos `delete`), auditoria sem texto nem título, `memories.version + 1`, `channels.version + 1`, jobs e idempotência numa só transação do `Store`, e chama `network_changed()` depois.
- A escrita nunca espera LLM ou embedding: persiste, enfileira `redact` e, com os embeddings ligados, `embed` (`{ memory_id, version }`; `title_only` no `revert`) e responde. Com `embedding.provider = off`, nada de `embed` e `embedding_state = off` (decidido no `core` por `content_jobs`, aplicado no `Store` pela presença do job).
- `content_hash` = SHA-256 de NFC + trim + espaços colapsados + minúsculo. Duplicata na criação devolve a existente (`duplicate: true`, `200`); na edição, no `revert` e no `move`, `409 MEMORY_DUPLICATE`.
- Sem mudança efetiva (edição, status repetido, `move` para o mesmo canal), nada é gravado.
- Canal arquivado: escrita → `409 CHANNEL_ARCHIVED`; leitura e `delete_memory` continuam.
- `delete_memory` mantém só a última revisão; `delete_channel` apaga memórias e revisões do canal.
- `memories.seq` é o rowid do `memory_fts` (triggers de insert/delete/update de `title`/`text`); nunca dependa de rowid implícito.
- Contadores nunca mudam `version` nem `updated_at`; `bump_injections` também avança `last_injected_at` (base do desuso).
- `Memory` expõe `stale_candidate` e `merged_into` (consolidador); `list_memories` filtra por `stale`; `restore_memory` limpa os dois. Edição de texto zera `importance`, `importance_pending` e `stale_candidate`.
- O texto das memórias nunca vai para logs, auditoria ou mensagens de erro.

Desenho: §6, §7, §9, §12 e §13. Decisões: `0011` e `0012`.
