# Spec - memórias

**Criado em**: 2026-10-30
**Status**: Implementada
**Versão alvo**: 0.4.0
**Input**: Desenho §6, §7 (exceto Redator e embeddings), §9 (`search_memories` textual), §12 (`categories`, `memory`), §13 (Memórias, `channel_stats`, configurações por canal), §14.

## Objetivo

Agentes e gestores gravam, editam, versionam, revertem, movem, arquivam, fixam, excluem e buscam memórias por texto nas três vias, com proveniência, histórico, auditoria, idempotência e controle de concorrência. A skill da CLI passa a existir.

## Escopo

- Tabelas `memories`, `memory_revisions`, `memory_fts` (com triggers) e `jobs` (só a fila; os workers chegam na 006).
- Operações de memória do §13 (exceto `import_memories`).
- `search_memories` só com FTS5/BM25, já com o contrato final.
- `similar` na resposta de escrita, calculado pelo FTS.
- Configurações `categories.*` e `memory.*`; `get_channel_settings` e `update_channel_settings`.
- `channel_stats`.
- `retrieval_count` e `last_retrieved_at` em lote.
- Skill `skills/engramind/SKILL.md` e guia `escrever-memorias`.

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | `create_memory { channel, text, category?, origin?, status? }` exige `write` efetivo no canal; canal arquivado → `409 CHANNEL_ARCHIVED`. |
| FR-002 | `text` entre `memory.text_min` e `memory.text_max` (sobreponível por canal) → senão `400 VALIDATION_ERROR` com `details.field`. |
| FR-003 | Categoria precisa existir em `categories.list` → senão `400 CATEGORY_NOT_FOUND`; padrão = primeira categoria da lista. |
| FR-004 | Com `memory.dedupe_exact`, texto com o mesmo `content_hash` normalizado no canal devolve a memória existente com `duplicate: true` e `200`, sem gravar nada. |
| FR-005 | Toda criação grava título provisório (primeira frase, até 100 caracteres), `title_state = pending`, `embedding_state = pending`, revisão 1, auditoria, `channels.version + 1` e enfileira `redact` e `embed` na mesma transação. |
| FR-006 | A resposta de escrita traz `similar`: até três memórias do mesmo canal com similaridade ≥ `memory.similar_threshold`. |
| FR-007 | `update_memory { id, text?, category?, expected_version? }`: versão divergente → `409 MEMORY_VERSION_CONFLICT` com `details.current_version`; edição zera `importance`, volta `title_state`/`embedding_state` para `pending` e reenfileira. |
| FR-008 | `archive`, `restore`, `pin`, `unpin`, `move`, `revert` e `delete` seguem as invariantes do §6: revisão (exceto `delete`), auditoria, versões da memória e do canal (nos dois canais em `move`), na mesma transação. |
| FR-009 | `move_memory` exige `write` efetivo na origem e no destino; duplicata exata no destino → `409 MEMORY_DUPLICATE` com `details.existing_id`. |
| FR-010 | `revert_memory { id, version }` cria nova revisão com o conteúdo da versão pedida; nunca altera revisões existentes. |
| FR-011 | `delete_memory` exige `delete` efetivo; remove memória, FTS e (na 007) vetor, e mantém só a última revisão. |
| FR-012 | `list_memories` filtra por `channels`, `categories`, `status`, `author`, `since`; pagina por cursor; `fields=title` devolve só `id`, `title`, `channel`, `category`, `status`. |
| FR-013 | `search_memories { query, channels?, categories?, limit?, include_archived?, exclude_ids?, fields? }` usa FTS5/BM25 sem acentos sobre os canais em modo `inject` e `search`, mais os `manage` citados explicitamente. |
| FR-014 | `get_memory` traz texto, autoria (`app`, `agent`), `origin`, contadores, estados e versão. `memory_history` lista as revisões com autor e motivo. |
| FR-015 | Texto de memória nunca aparece em logs `INFO`+, auditoria (só hashes e metadados) ou mensagens de erro. |
| FR-016 | A skill documenta só comandos existentes em `engramind help --json` (teste de contrato). |

## Fora do escopo

- Redator e embeddings (os jobs ficam na fila até a 006/007).
- Ramo vetorial da busca, `get_hints`, `exclude_context` (005/007).
- Webhooks (008) e `import_memories` (008).

## Critérios de sucesso

- Um agente curador executa o fluxo do §5 do desenho (listar canais, listar com `fields`, criar com `origin`) pela CLI seguindo só a skill.
- Busca por "configuracao" encontra "configuração".
- Conflito de versão, idempotência, duplicata, `revert` e `move` cobertos por testes nas três vias.
