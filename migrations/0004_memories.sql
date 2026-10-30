-- Etapa 004: memórias, revisões, busca textual e a fila de jobs.
-- Ids públicos: m_ + 8 hex. `seq` é o rowid explícito: o FTS externo depende dele, e um rowid
-- implícito pode mudar num VACUUM. Revisões e autoria não têm chave estrangeira: a última
-- revisão sobrevive ao `delete_memory` e a autoria sobrevive à exclusão do app ou do agente.

CREATE TABLE memories (
    seq               INTEGER PRIMARY KEY,
    id                TEXT NOT NULL UNIQUE,
    channel_id        TEXT NOT NULL REFERENCES channels (id),
    category          TEXT NOT NULL,
    title             TEXT NOT NULL,
    text              TEXT NOT NULL,
    status            TEXT NOT NULL CHECK (status IN ('active', 'permanent', 'archived')),
    importance        REAL,
    content_hash      BLOB NOT NULL,
    version           INTEGER NOT NULL DEFAULT 1,
    title_state       TEXT NOT NULL CHECK (title_state IN ('pending', 'ready', 'fallback')),
    embedding_state   TEXT NOT NULL CHECK (embedding_state IN ('pending', 'ready', 'failed', 'off')),
    author_app_id     TEXT NOT NULL,
    author_agent_id   TEXT,
    origin_source     TEXT,
    origin_label      TEXT,
    retrieval_count   INTEGER NOT NULL DEFAULT 0,
    injected_count    INTEGER NOT NULL DEFAULT 0,
    last_retrieved_at TEXT,
    verified_at       TEXT,
    merged_into       TEXT,
    stale_candidate   INTEGER NOT NULL DEFAULT 0 CHECK (stale_candidate IN (0, 1)),
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    archived_at       TEXT
) STRICT;

CREATE INDEX memories_channel_status ON memories (channel_id, status);
CREATE INDEX memories_channel_hash ON memories (channel_id, content_hash);
CREATE INDEX memories_updated ON memories (updated_at, id);
CREATE INDEX memories_category ON memories (category);

CREATE TABLE memory_revisions (
    id         INTEGER PRIMARY KEY,
    memory_id  TEXT NOT NULL,
    version    INTEGER NOT NULL,
    title      TEXT NOT NULL,
    text       TEXT NOT NULL,
    category   TEXT NOT NULL,
    status     TEXT NOT NULL,
    app_id     TEXT NOT NULL,
    agent_id   TEXT,
    reason     TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (memory_id, version)
) STRICT;

CREATE VIRTUAL TABLE memory_fts USING fts5 (
    title,
    text,
    content = 'memories',
    content_rowid = 'seq',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER memories_fts_insert AFTER INSERT ON memories BEGIN
    INSERT INTO memory_fts (rowid, title, text) VALUES (new.seq, new.title, new.text);
END;

CREATE TRIGGER memories_fts_delete AFTER DELETE ON memories BEGIN
    INSERT INTO memory_fts (memory_fts, rowid, title, text)
    VALUES ('delete', old.seq, old.title, old.text);
END;

CREATE TRIGGER memories_fts_update AFTER UPDATE OF title, text ON memories BEGIN
    INSERT INTO memory_fts (memory_fts, rowid, title, text)
    VALUES ('delete', old.seq, old.title, old.text);
    INSERT INTO memory_fts (rowid, title, text) VALUES (new.seq, new.title, new.text);
END;

CREATE TABLE jobs (
    id           INTEGER PRIMARY KEY,
    kind         TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    state        TEXT NOT NULL DEFAULT 'queued'
                 CHECK (state IN ('queued', 'running', 'done', 'failed')),
    attempts     INTEGER NOT NULL DEFAULT 0,
    run_after    TEXT NOT NULL,
    locked_until TEXT,
    last_error   TEXT,
    created_at   TEXT NOT NULL,
    finished_at  TEXT
) STRICT;

CREATE INDEX jobs_ready ON jobs (state, run_after);
