-- Long-term memory store with full-text search.
CREATE TABLE IF NOT EXISTS memories (
    id          VARCHAR      NOT NULL   COMMENT 'ULID primary key',
    user_id     VARCHAR      NOT NULL   COMMENT 'Owner',
    scope       VARCHAR      NOT NULL DEFAULT 'user' COMMENT 'user | shared | session',
    session_id  VARCHAR      NULL       COMMENT 'Set when scope = session',
    key         VARCHAR      NOT NULL   COMMENT 'Human-readable identifier',
    content     TEXT         NOT NULL   COMMENT 'Memory content (Markdown)',
    created_at  TIMESTAMP    NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMP    NOT NULL DEFAULT NOW(),

    INVERTED INDEX idx_content(content)
) COMMENT = 'Long-term memory with FTS search';
