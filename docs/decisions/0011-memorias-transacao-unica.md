# 0011 — Memórias: transação única, duplicata e fila antes dos workers

**Data**: 2026-10-30 · **Etapa**: 004 (0.4.0) · **Status**: aceita

## Contexto

O desenho (§6, §7) exige que toda mutação de memória grave revisão, auditoria, as versões da memória e do canal e enfileire o trabalho na mesma transação, e que a escrita nunca espere o LLM. Mas deixa abertos o formato da fila antes dos workers, a duplicata na edição, o efeito de um canal arquivado e o que fazer com uma memória num canal que o token não alcança.

## Decisão

- **Transação única no `Store`.** Cada mutação (`create`, `update`, `archive`, `restore`, `pin`, `unpin`, `move`, `revert`, `delete`) grava, numa só transação do escritor: a memória, a revisão (exceto `delete`), a auditoria, `memories.version + 1`, `channels.version + 1` (nos dois canais em `move`), os jobs e a idempotência. O `core` valida antes e passa o resultado pronto (inclusive `similar`), para a resposta guardada na idempotência ser a final.
- **Alvo conferido.** As mutações recebem `MemoryTarget { id, channel_id }`; se a memória mudou de canal entre a checagem de acesso e a escrita, a escrita responde `MEMORY_NOT_FOUND`.
- **Fila antes dos workers (Q-001).** `redact` e `embed` são enfileirados desde a 004, com payload `{ memory_id, version }` (`title_only: true` no `revert`). Webhooks só a partir da 008.
- **Duplicata.** `content_hash` = SHA-256 do texto em NFC, sem espaços nas pontas, com espaços colapsados e minúsculo. Na criação, com `memory.dedupe_exact`, um texto igual a qualquer memória do canal (em qualquer status) devolve a existente com `duplicate: true` e `200`, sem gravar a memória; a idempotência guarda a resposta com status 200. Na edição, no `revert` e no destino do `move`, a colisão responde `409 MEMORY_DUPLICATE` com `details.existing_id`.
- **Edição (Q-007).** Sem mudança efetiva, nada é gravado. Só a categoria: revisão e versões, sem reprocessar. Texto novo: título provisório, `title_state`/`embedding_state` `pending`, `importance` zerada e jobs.
- **Revert (Q-008).** Restaura texto, título e categoria da revisão pedida numa revisão nova, com `expected_version` igual à versão lida. Se o texto não muda, o título também fica.
- **Status.** Repetir um status que já vale não grava nada. `restore` só tira de `archived`; `unpin` só tira de `permanent`.
- **Canal arquivado (Q-006).** Escrita → `409 CHANNEL_ARCHIVED`; leitura e `delete_memory` continuam.
- **Memória fora do alcance (Q-005).** Qualquer operação por id numa memória cujo canal o token não alcança (ou fora do `Engramind-Channels`) responde `404 MEMORY_NOT_FOUND`. Sem o escopo no canal alcançado, `403 SCOPE_REQUIRED`.
- **`delete_memory`** mantém só a última revisão (sem chave estrangeira em `memory_revisions`); `delete_channel` apaga as memórias e todas as revisões delas, com `memories_removed` na auditoria.
- **Auditoria sem texto.** A auditoria de memória guarda id, canal, categoria, status, versão, `content_hash`, estados e `origin_source`; nunca o texto nem o título.
- **`seq` explícito.** `memories` tem `seq INTEGER PRIMARY KEY` (rowid do FTS externo) e `id TEXT UNIQUE`: um rowid implícito pode mudar num `VACUUM` e dessincronizar o `memory_fts`.
- **Cache da rede.** Como toda mutação de memória incrementa `channels.version`, ela também invalida o cache da rede, para `get_channel` mostrar a versão atual.
- **Contadores.** `retrieval_count`/`last_retrieved_at` acumulam em memória e são gravados a cada 5 s, sem mudar `version` nem `updated_at`.

## Consequências

- A 006 consome `redact` e `embed` e descarta jobs de versões superadas.
- Um texto arquivado ainda bloqueia uma nova memória igual no canal (a resposta mostra a arquivada, que pode ser restaurada).
- Nova dependência: `unicode-normalization` (NFC no hash e remoção de acentos nos termos).
