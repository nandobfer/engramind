-- 006: cofre de segredos, registro das chamadas ao LLM e o resultado dos jobs.
-- `value_enc` é `v1 || nonce(12) || ciphertext` (AES-256-GCM com `SECRETS_KEY`, decisão 0015).
-- `llm_calls` nunca guarda o texto, o prompt nem a chave; `at` é RFC 3339 UTC ordenável.

CREATE TABLE secrets (
    key        TEXT PRIMARY KEY,
    value_enc  BLOB NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE llm_calls (
    id            INTEGER PRIMARY KEY,
    at            TEXT NOT NULL,
    purpose       TEXT NOT NULL,
    provider      TEXT NOT NULL,
    model         TEXT NOT NULL,
    input_tokens  INTEGER,
    output_tokens INTEGER,
    cost_usd      REAL,
    duration_ms   INTEGER NOT NULL,
    outcome       TEXT NOT NULL,
    error         TEXT
) STRICT;

CREATE INDEX llm_calls_at ON llm_calls (at);

-- `done` com `outcome`: `applied`, `title_only`, `superseded` ou `skipped`.
ALTER TABLE jobs ADD COLUMN outcome TEXT;

CREATE INDEX jobs_kind_ready ON jobs (kind, state, run_after);
