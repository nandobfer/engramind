-- Etapa 002: identidades, configurações, idempotência e auditoria.
-- Ids públicos: app_/ag_/tk_ + 8 hex. Carimbos RFC 3339 UTC com milissegundos.

CREATE TABLE apps (
    id          TEXT PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    description TEXT,
    created_at  TEXT NOT NULL,
    disabled_at TEXT
) STRICT;

CREATE TABLE agents (
    id          TEXT PRIMARY KEY,
    app_id      TEXT NOT NULL REFERENCES apps (id) ON DELETE CASCADE,
    slug        TEXT NOT NULL,
    name        TEXT NOT NULL,
    description TEXT,
    created_at  TEXT NOT NULL,
    disabled_at TEXT,
    UNIQUE (app_id, slug)
) STRICT;

CREATE INDEX agents_created ON agents (created_at, id);

CREATE TABLE tokens (
    id           TEXT PRIMARY KEY,
    app_id       TEXT NOT NULL REFERENCES apps (id) ON DELETE CASCADE,
    agent_id     TEXT REFERENCES agents (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('agent', 'admin')),
    scopes       TEXT NOT NULL,
    label        TEXT,
    secret_hash  BLOB NOT NULL,
    expires_at   TEXT,
    created_at   TEXT NOT NULL,
    last_used_at TEXT,
    revoked_at   TEXT,
    CHECK (kind = 'admin' OR agent_id IS NOT NULL)
) STRICT;

CREATE INDEX tokens_app ON tokens (app_id);
CREATE INDEX tokens_agent ON tokens (agent_id);
CREATE INDEX tokens_created ON tokens (created_at, id);

CREATE TABLE settings (
    key            TEXT PRIMARY KEY,
    value_json     TEXT NOT NULL,
    updated_by_app TEXT,
    updated_at     TEXT NOT NULL
) STRICT;

CREATE TABLE idempotency (
    token_id      TEXT NOT NULL,
    key           TEXT NOT NULL,
    request_hash  BLOB NOT NULL,
    response_json TEXT NOT NULL,
    status        INTEGER NOT NULL,
    created_at    TEXT NOT NULL,
    PRIMARY KEY (token_id, key)
) STRICT;

CREATE INDEX idempotency_created ON idempotency (created_at);

-- Sem chaves estrangeiras: a auditoria sobrevive à exclusão de apps, agentes e tokens.
CREATE TABLE audit_log (
    id          INTEGER PRIMARY KEY,
    at          TEXT NOT NULL,
    app_id      TEXT,
    agent_id    TEXT,
    token_id    TEXT,
    action      TEXT NOT NULL,
    entity      TEXT NOT NULL,
    entity_id   TEXT,
    before_json TEXT,
    after_json  TEXT
) STRICT;

CREATE INDEX audit_entity ON audit_log (entity, entity_id);
