-- Etapa 009: consolidação.
-- `importance_pending` guarda a nota do consolidador até a próxima transação que incrementa
-- `channels.version` do canal, para a ordem do contexto nunca mudar sem mudar a `version`
-- (D-001). `last_injected_at` é o instante do último lote de injeções (base do desuso, Q-003).

ALTER TABLE memories ADD COLUMN importance_pending REAL;
ALTER TABLE memories ADD COLUMN last_injected_at TEXT;

-- Pares que o Redator recusou fundir (Q-014); valem enquanto os dois textos forem os mesmos.
CREATE TABLE consolidation_pairs (
    memory_a   TEXT NOT NULL REFERENCES memories (id) ON DELETE CASCADE,
    memory_b   TEXT NOT NULL REFERENCES memories (id) ON DELETE CASCADE,
    hash_a     BLOB NOT NULL,
    hash_b     BLOB NOT NULL,
    decided_at TEXT NOT NULL,
    PRIMARY KEY (memory_a, memory_b),
    CHECK (memory_a < memory_b)
) STRICT;

CREATE INDEX consolidation_pairs_b ON consolidation_pairs (memory_b);
