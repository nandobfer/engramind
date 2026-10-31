-- Embeddings (007): um vetor por memória; reindexar substitui o anterior. `model_id` separa
-- modelos e dimensões (a busca só compara o modelo vigente), `updated_at` marca o progresso de
-- `reembed_all` e a cascata faz `delete_memory` apagar o vetor na mesma transação.
CREATE TABLE memory_vectors (
    memory_id  TEXT PRIMARY KEY REFERENCES memories (id) ON DELETE CASCADE,
    model_id   TEXT NOT NULL,
    dims       INTEGER NOT NULL CHECK (dims > 0),
    embedding  BLOB NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX memory_vectors_model ON memory_vectors (model_id, updated_at);
