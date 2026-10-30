# Contracts - memórias

**Criado em**: 2026-10-30
**Status**: Draft
**Feature**: [`spec.md`](spec.md)

## Migração `0004_memories.sql`

```sql
memories (id TEXT PK, channel_id FK→channels, category, title, text,
          status CHECK IN ('active','permanent','archived'), importance REAL NULL,
          content_hash BLOB, version INTEGER, title_state CHECK IN ('pending','ready','fallback'),
          embedding_state CHECK IN ('pending','ready','failed','off'),
          author_app_id, author_agent_id NULL, origin_source NULL, origin_label NULL,
          retrieval_count INTEGER DEFAULT 0, injected_count INTEGER DEFAULT 0,
          last_retrieved_at NULL, verified_at NULL, merged_into NULL, stale_candidate INTEGER DEFAULT 0,
          created_at, updated_at, archived_at NULL)
CREATE INDEX memories_channel_status ON memories(channel_id, status);
CREATE INDEX memories_channel_hash ON memories(channel_id, content_hash);
memory_revisions (id INTEGER PK, memory_id TEXT, version, title, text, category, status,
                  app_id, agent_id NULL, reason, created_at, UNIQUE(memory_id, version))
CREATE VIRTUAL TABLE memory_fts USING fts5(title, text, content='memories', content_rowid='rowid',
                                           tokenize='unicode61 remove_diacritics 2');
-- triggers AFTER INSERT/UPDATE/DELETE em memories mantendo memory_fts
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

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal |
|---|---|---|---|
| `categories.list` | lista `{ key, label, priority }` | `PROJECT_RULES`, `ARCHITECTURE`, `CONSTRAINTS`, `CONFIG_VALUES`, `NAMING` | não |
| `memory.text_min` | inteiro | 3 | não |
| `memory.text_max` | inteiro | 4000 | sim |
| `memory.dedupe_exact` | booleano | true | não |
| `memory.similar_threshold` | número 0–1 | 0.85 | não |

## Códigos de erro novos

| Código | Status |
|---|---|
| `MEMORY_NOT_FOUND`, `REVISION_NOT_FOUND` | 404 |
| `CATEGORY_NOT_FOUND` | 400 |
| `MEMORY_VERSION_CONFLICT`, `MEMORY_DUPLICATE`, `CATEGORY_IN_USE` | 409 |

## Skill

`skills/engramind/SKILL.md`: quando ler (`engramind context`, a partir da 005) e buscar (`engramind memories search`), como escrever uma boa memória (fato durável, genérico, sem dados sensíveis, códigos de tarefa ou URLs temporárias; checar `similar` e preferir `update`), comandos com `--json` e tratamento dos códigos de erro comuns.
