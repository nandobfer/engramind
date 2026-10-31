-- 005: registro de uso de `get_context` (entregue) e `search_memories` (desenho §6).
-- `at` é RFC 3339 em UTC com milissegundos: ordenável como texto, o que permite filtrar por
-- período (`at >= ? AND at < ?`) e agrupar por dia (`substr(at, 1, 10)`).

CREATE TABLE usage_log (
    id              INTEGER PRIMARY KEY,
    at              TEXT NOT NULL,
    app_id          TEXT NOT NULL,
    agent_id        TEXT,
    token_id        TEXT NOT NULL,
    op              TEXT NOT NULL,
    -- Listas JSON de ids (`["ch_…"]`, `["m_…"]`).
    channel_ids     TEXT NOT NULL,
    memory_ids      TEXT NOT NULL,
    rendered_tokens INTEGER
);

CREATE INDEX usage_log_at ON usage_log (at);
CREATE INDEX usage_log_agent_at ON usage_log (agent_id, at);
CREATE INDEX usage_log_op_at ON usage_log (op, at);
