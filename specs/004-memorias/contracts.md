# Contracts - memórias

**Criado em**: 2026-10-30
**Status**: Implementado
**Feature**: [`spec.md`](spec.md)

## Migração `0004_memories.sql`

```sql
memories (seq INTEGER PK, id TEXT UNIQUE, channel_id FK→channels, category, title, text,
          status CHECK IN ('active','permanent','archived'), importance REAL NULL,
          content_hash BLOB, version INTEGER, title_state CHECK IN ('pending','ready','fallback'),
          embedding_state CHECK IN ('pending','ready','failed','off'),
          author_app_id, author_agent_id NULL, origin_source NULL, origin_label NULL,
          retrieval_count INTEGER DEFAULT 0, injected_count INTEGER DEFAULT 0,
          last_retrieved_at NULL, verified_at NULL, merged_into NULL, stale_candidate INTEGER DEFAULT 0,
          created_at, updated_at, archived_at NULL)
CREATE INDEX memories_channel_status ON memories(channel_id, status);
CREATE INDEX memories_channel_hash ON memories(channel_id, content_hash);
CREATE INDEX memories_updated ON memories(updated_at, id);
CREATE INDEX memories_category ON memories(category);
memory_revisions (id INTEGER PK, memory_id TEXT, version, title, text, category, status,
                  app_id, agent_id NULL, reason, created_at, UNIQUE(memory_id, version))
CREATE VIRTUAL TABLE memory_fts USING fts5(title, text, content='memories', content_rowid='seq',
                                           tokenize='unicode61 remove_diacritics 2');
-- triggers AFTER INSERT, AFTER DELETE e AFTER UPDATE OF title, text em memories mantendo memory_fts
jobs (id INTEGER PK, kind, payload_json, state CHECK IN ('queued','running','done','failed'),
      attempts INTEGER DEFAULT 0, run_after, locked_until NULL, last_error NULL, created_at, finished_at NULL)
CREATE INDEX jobs_ready ON jobs(state, run_after);
```

## Operações

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `list_memories` | any·read | `GET /v1/memories` | `engramind memories list` |
| `get_memory` | any·read | `GET /v1/memories/{memory}` | `engramind memories get <id>` |
| `search_memories` | any·read | `POST /v1/memories/search` | `engramind memories search <consulta>` |
| `memory_history` | any·read | `GET /v1/memories/{memory}/history` | `engramind memories history <id>` |
| `create_memory` | any·write | `POST /v1/memories` | `engramind memories create` |
| `update_memory` | any·write | `PATCH /v1/memories/{memory}` | `engramind memories update <id>` |
| `archive_memory` | any·write | `POST /v1/memories/{memory}/archive` | `engramind memories archive <id>` |
| `restore_memory` | any·write | `POST /v1/memories/{memory}/restore` | `engramind memories restore <id>` |
| `pin_memory` | any·write | `POST /v1/memories/{memory}/pin` | `engramind memories pin <id>` |
| `unpin_memory` | any·write | `POST /v1/memories/{memory}/unpin` | `engramind memories unpin <id>` |
| `move_memory` | any·write (origem e destino) | `POST /v1/memories/{memory}/move` | `engramind memories move <id> --to <canal>` |
| `revert_memory` | any·write | `POST /v1/memories/{memory}/revert` | `engramind memories revert <id> --version <n>` |
| `delete_memory` | any·delete | `DELETE /v1/memories/{memory}` | `engramind memories delete <id>` |
| `channel_stats` | any·read | `GET /v1/channels/{channel}/stats` | `engramind channels stats <canal>` |
| `get_channel_settings` | admin·read | `GET /v1/channels/{channel}/settings` | `engramind channels settings get <canal>` |
| `update_channel_settings` | admin·write | `PUT /v1/channels/{channel}/settings` | `engramind channels settings set <canal> <chave> <valor>` |

```jsonc
// POST /v1/memories   (Idempotency-Key opcional)
{ "channel": "eng/global", "text": "…", "category": "CONSTRAINTS",
  "origin": { "source": "curator", "label": "caso 123" }, "status": "active" }
// 201 (criada) | 200 (duplicate: true)
{ "memory": { "id": "m_8f2a1c3d", "channel": "eng/global", "category": "CONSTRAINTS",
              "title": "…", "title_state": "pending", "status": "active", "version": 1, ... },
  "duplicate": false,
  "similar": [ { "id": "m_…", "title": "…", "score": 0.91 } ] }
```

```jsonc
// PATCH /v1/memories/{memory}   → 200 MemoryWrite
{ "text": "…", "category": "NAMING", "expected_version": 3 }
// POST /v1/memories/{memory}/move    { "to": "eng/projeto-atlas" }   → 200 Memory
// POST /v1/memories/{memory}/revert  { "version": 1 }                → 200 Memory
// archive | restore | pin | unpin (POST, sem corpo) e DELETE         → 200 Memory
// POST /v1/memories/search (leitura, sem Idempotency-Key)
{ "query": "testes locais", "channels": ["eng/global"], "categories": ["CONSTRAINTS"],
  "limit": 10, "include_archived": false, "exclude_ids": [], "fields": "full" }
// 200
{ "results": [ { "rank": 1, "score": 0.016393, "memory": { "id": "m_…", … } } ] }
// GET /v1/memories?channels=&categories=&status=&author=&since=&fields=&cursor=&limit=
{ "memories": [ … ], "next_cursor": null }
// GET /v1/memories/{memory}/history?cursor=&limit=
{ "memory_id": "m_…", "revisions": [ { "version": 2, "title": "…", "text": "…", "category": "…",
  "status": "active", "author": { … }, "reason": "update", "created_at": "…" } ], "next_cursor": null }
// GET /v1/channels/{channel}/stats
{ "channel": "eng/global", "channel_id": "ch_…", "version": 7,
  "memories": { "total": 2, "active": 1, "permanent": 1, "archived": 0 },
  "by_category": { "NAMING": 1 }, "pending": { "title": 2, "embedding": 2 },
  "retrievals": 4, "last_memory_at": "…" }
// GET|PUT /v1/channels/{channel}/settings   (PUT: { "memory.text_max": 2000 }, null remove)
{ "channel": "eng/global", "channel_id": "ch_…", "settings": [ { "key": "memory.text_max",
  "value": 2000, "global": 4000, "override": 2000, "type": "integer", "min": 10, "max": 20000,
  "description": "…" } ] }
```

`Memory` completa: `id`, `channel`, `channel_id`, `category`, `title`, `title_state`, `text`, `status`, `importance`, `version`, `embedding_state`, `author { app_id, app, agent_id, agent }`, `origin { source, label }`, `retrieval_count`, `injected_count`, `last_retrieved_at`, `verified_at`, `created_at`, `updated_at`, `archived_at`. Com `fields=title`: `id`, `title`, `channel`, `category`, `status`.

MCP: argumentos planos (path + query + corpo) e `idempotency_key` nas mutações; `update_channel_settings` recebe `{ channel, settings, idempotency_key }`. CLI: `--text -` lê da entrada padrão; `memories create --origin-source --origin-label`; `memories update --expected-version`; o `--channels` global vira o cabeçalho.

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal |
|---|---|---|---|
| `categories.list` | lista `{ key, label, priority }` | `PROJECT_RULES`, `ARCHITECTURE`, `CONSTRAINTS`, `CONFIG_VALUES`, `NAMING` | não |
| `memory.text_min` | inteiro (1 a 1000) | 3 | não |
| `memory.text_max` | inteiro (10 a 20000) | 4000 | sim |
| `memory.dedupe_exact` | booleano | true | não |
| `memory.similar_threshold` | número 0–1 | 0.85 | não |

## Códigos de erro novos

| Código | Status |
|---|---|
| `MEMORY_NOT_FOUND`, `REVISION_NOT_FOUND` | 404 |
| `CATEGORY_NOT_FOUND` | 400 |
| `MEMORY_VERSION_CONFLICT`, `MEMORY_DUPLICATE`, `CATEGORY_IN_USE`, `CHANNEL_ARCHIVED` | 409 |

Erro local da CLI: `CLI_INPUT_INVALID` (saída 2), falha ao ler `--text -`.

## Skill

`skills/engramind/SKILL.md`: quando ler o contexto (`get_context` chega na 005; a skill ainda não o cita) e buscar (`engramind memories search`), como escrever uma boa memória (fato durável, genérico, sem dados sensíveis, códigos de tarefa ou URLs temporárias; checar `similar` e preferir `update`), comandos com `--json` e tratamento dos códigos de erro comuns.
