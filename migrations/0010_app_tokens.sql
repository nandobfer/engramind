-- 0.11.0: token de app (`kind = 'app'`, vinculado a um app e sem agente; decisão 0028).
-- O SQLite não altera CHECK: a tabela é recriada com os mesmos dados. Nenhuma chave
-- estrangeira aponta para `tokens`, então a troca é segura com `foreign_keys = ON`.

CREATE TABLE tokens_new (
    id           TEXT PRIMARY KEY,
    app_id       TEXT NOT NULL REFERENCES apps (id) ON DELETE CASCADE,
    agent_id     TEXT REFERENCES agents (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('agent', 'admin', 'app')),
    scopes       TEXT NOT NULL,
    label        TEXT,
    secret_hash  BLOB NOT NULL,
    expires_at   TEXT,
    created_at   TEXT NOT NULL,
    last_used_at TEXT,
    revoked_at   TEXT,
    CHECK (kind <> 'agent' OR agent_id IS NOT NULL),
    CHECK (kind <> 'app' OR agent_id IS NULL)
) STRICT;

INSERT INTO tokens_new
    (id, app_id, agent_id, kind, scopes, label, secret_hash, expires_at, created_at,
     last_used_at, revoked_at)
SELECT id, app_id, agent_id, kind, scopes, label, secret_hash, expires_at, created_at,
       last_used_at, revoked_at
FROM tokens;

DROP TABLE tokens;

ALTER TABLE tokens_new RENAME TO tokens;

CREATE INDEX tokens_app ON tokens (app_id);
CREATE INDEX tokens_agent ON tokens (agent_id);
CREATE INDEX tokens_created ON tokens (created_at, id);
