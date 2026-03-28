-- Shared memory store (lives in evotai_meta database, shared across all agents).
CREATE TABLE IF NOT EXISTS memory (
    id               VARCHAR      NOT NULL   COMMENT 'ULID primary key',
    user_id          VARCHAR      NOT NULL   COMMENT 'Owner user ID',
    agent_id         VARCHAR      NOT NULL DEFAULT '' COMMENT 'Agent that created this memory',
    scope            VARCHAR      NOT NULL DEFAULT 'agent' COMMENT 'agent | shared',
    key              VARCHAR      NOT NULL   COMMENT 'Human-readable identifier',
    content          TEXT         NOT NULL   COMMENT 'Memory content (Markdown)',
    access_count     INT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'Number of times accessed',
    last_accessed_at TIMESTAMP    NOT NULL DEFAULT NOW() COMMENT 'Last access time',
    created_at       TIMESTAMP    NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMP    NOT NULL DEFAULT NOW(),

    INVERTED INDEX idx_content(content)
) COMMENT = 'Shared memory with FTS search (cross-agent)';
