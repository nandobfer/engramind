# 0032 — Título do cliente, tags, chave natural e origem da última edição

- Status: aceita (0.11.0)
- Contexto: desenho §7; decisões 0013 (memórias) e 0016 (Redator).

## Decisão

- **Título do cliente**: `title` (3 a 200, sem controles) em `create_memory`, `update_memory` e na
  importação grava o título `ready` e travado (`title_locked`). O Redator não troca título travado
  (sem revisão de texto, o job termina `locked` sem chamar o LLM); a fusão do consolidador mantém o
  título travado do alvo. `title_locked: false` destrava e enfileira o `redact` `title_only`; com
  `title` no mesmo corpo → `400`.
- **Tags**: até 20 de 1 a 40 (trim e repetidas removidas), na tabela `memory_tags` com a posição e
  no JSON de cada revisão. Fora do `content_hash` e sem jobs; o filtro é E lógico (`EXISTS` por tag)
  em `list_memories`, `search_memories` (ramos textual e vetorial).
- **Chave natural**: `key` (1 a 80, `[A-Za-z0-9:_-]`), única por canal (índice parcial
  `UNIQUE (channel_id, key) WHERE key IS NOT NULL`) e imutável. Sem `upsert`, repetida →
  `409 MEMORY_KEY_EXISTS`; com `upsert`, a mesma transação aplica a edição (revisão só se algo mudou,
  `created: false`, `200`). Com `key`, o dedupe exato por texto não se aplica: a chave é a identidade.
  O escritor único serializa upserts concorrentes. Upsert (criação ou importação) sobre memória
  arquivada a reativa no status pedido; `move_memory` com a `key` já usada no destino →
  `409 MEMORY_KEY_EXISTS`. Na importação, um item com chave existente vai
  para `updated[]`.
- **Origem**: cada revisão guarda a origem informada; `memories.last_origin_*` muda só quando uma
  origem é informada (criação, edição, importação ou `revert` de revisão com origem). Sem backfill:
  registros antigos ficam com `last_origin: null`.

## Consequências

- `revert_memory` restaura título, trava e tags.
- Upsert sem `expected_version` é "última escrita vence".
