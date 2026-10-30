# Use quando desenvolver persistência

## `Store`

- Todo acesso a dados passa pela interface `Store`. Só a implementação SQLite conhece SQL e `rusqlite`.
- O contrato do `Store` não pode depender de particularidades do SQLite, para permitir a implementação Postgres + pgvector futura.
- Modelo de dados em §6 do desenho. Ids públicos curtos com prefixo de tipo (`m_`, `ch_`, `ag_`, `app_`, `tk_`).

## SQLite

- Arquivo único (`DATABASE_PATH`, `/data/engramind.db` no contêiner).
- PRAGMAs: `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`, `foreign_keys=ON`.
- Uma thread escritora recebe comandos por canal `tokio` e serializa as escritas; um pool de leitores atende consultas. Nunca escreva fora da thread escritora.
- Contadores (`retrieval_count`, `injected_count`, `last_used_at`, `last_retrieved_at`) são acumulados em memória e gravados em lote.
- Nunca monte o banco em NFS, pasta de rede ou bind mount compartilhado entre hosts.

## Migrações

- SQL versionado em `migrations/`, embutido no binário (`rusqlite_migration`).
- Nunca edite nem remova uma migração que possa ter sido aplicada; corrija com uma nova.
- `GET /health/ready` e `engramind doctor` conferem migrações aplicadas.

## FTS5 e sqlite-vec

- `memory_fts` é FTS5 externa sobre `title` e `text`, tokenizer `unicode61 remove_diacritics 2`, sincronizada por trigger.
- `memory_vectors` guarda `model_id`, `dims` e o vetor float32. Nunca misture dimensões ou modelos numa mesma comparação.
- O registro do `sqlite-vec` (`sqlite3_auto_extension`) é o único bloco `unsafe` do crate, isolado num módulo com `#[allow(unsafe_code)]` e comentário de justificativa.
- A busca vetorial começa com varredura exata filtrada; `vec0` particionado por canal só na fase 5, sem mudar o contrato.

## Invariantes de escrita

Todo `create`, `update`, `archive`, `restore`, `pin`, `move`, `revert` e `delete` de memória, na mesma transação:

- grava `memory_revisions` (exceto `delete`, que preserva a última);
- grava `audit_log` (sem segredos);
- incrementa `memories.version` e `channels.version`;
- enfileira o webhook e os jobs aplicáveis.

Outras regras:

- Duplicata exata (`content_hash` normalizado) no canal devolve a existente com `duplicate: true`.
- `revert_memory` cria nova revisão; nunca reescreve histórico.
- `idempotency` guarda `(token_id, key, request_hash, response_json)`; mesma chave com corpo diferente é conflito.
- Fila `jobs`: captura com `UPDATE … RETURNING` e `locked_until`, backoff exponencial.
- Retenção configurável para `usage_log`, `llm_calls` e `idempotency`.

## Testes

- Um SQLite temporário por teste (arquivo em `tempdir`, extensões carregadas).
- Cubra migrações do zero, invariantes de escrita, FTS sem acentos, conflito de versão, idempotência e fila com retentativa.
