-- 0.11.0: origem da última edição, título do cliente, chave natural e tags (decisão 0032).
-- Registros antigos ficam com `last_origin` nulo, título destravado, sem chave e sem tags.

ALTER TABLE memories ADD COLUMN last_origin_source TEXT;
ALTER TABLE memories ADD COLUMN last_origin_label TEXT;
ALTER TABLE memories ADD COLUMN title_locked INTEGER NOT NULL DEFAULT 0
    CHECK (title_locked IN (0, 1));
ALTER TABLE memories ADD COLUMN key TEXT;

-- A chave é única no canal; memórias sem chave não entram no índice.
CREATE UNIQUE INDEX memories_channel_key ON memories (channel_id, key) WHERE key IS NOT NULL;

ALTER TABLE memory_revisions ADD COLUMN origin_source TEXT;
ALTER TABLE memory_revisions ADD COLUMN origin_label TEXT;
ALTER TABLE memory_revisions ADD COLUMN title_locked INTEGER NOT NULL DEFAULT 0;
-- Lista JSON das tags da revisão (histórico e `revert_memory`).
ALTER TABLE memory_revisions ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';

CREATE TABLE memory_tags (
    memory_id TEXT NOT NULL REFERENCES memories (id) ON DELETE CASCADE,
    tag       TEXT NOT NULL,
    position  INTEGER NOT NULL,
    PRIMARY KEY (memory_id, tag)
) STRICT;

CREATE INDEX memory_tags_tag ON memory_tags (tag, memory_id);
