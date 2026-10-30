-- Etapa 003: canais e assinaturas.
-- Ids públicos: ch_/sub_ + 8 hex. `subscriptions.target` guarda o slug exato ou o prefixo sem o
-- `*` final (`eng/` para `eng/*`, `eng/projeto-` para `eng/projeto-*`).

CREATE TABLE channels (
    id            TEXT PRIMARY KEY,
    slug          TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    description   TEXT,
    priority      INTEGER NOT NULL DEFAULT 0,
    settings_json TEXT NOT NULL DEFAULT '{}',
    version       INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL,
    archived_at   TEXT
) STRICT;

CREATE TABLE subscriptions (
    id         TEXT PRIMARY KEY,
    agent_id   TEXT NOT NULL REFERENCES agents (id) ON DELETE CASCADE,
    target     TEXT NOT NULL,
    is_prefix  INTEGER NOT NULL CHECK (is_prefix IN (0, 1)),
    mode       TEXT NOT NULL CHECK (mode IN ('inject', 'search', 'manage')),
    access     TEXT,
    priority   INTEGER,
    created_at TEXT NOT NULL,
    UNIQUE (agent_id, target, is_prefix)
) STRICT;

CREATE INDEX subscriptions_target ON subscriptions (target, is_prefix);
