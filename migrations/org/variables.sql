-- User-level variables (shared across agents, lives in evotai_meta).
CREATE TABLE IF NOT EXISTS variables (
    id            VARCHAR      NOT NULL   COMMENT 'ULID primary key',
    key           VARCHAR      NOT NULL   COMMENT 'Variable name (unique per user)',
    value         TEXT         NOT NULL DEFAULT '' COMMENT 'Variable value',
    secret        BOOLEAN      NOT NULL DEFAULT false COMMENT 'Masked in API responses',
    revoked       BOOLEAN      NOT NULL DEFAULT false COMMENT 'Soft-revoke without deleting',
    user_id       VARCHAR      NOT NULL   COMMENT 'Owner user ID',
    scope         VARCHAR      NOT NULL DEFAULT 'shared' COMMENT 'private | shared',
    created_by    VARCHAR      NOT NULL DEFAULT '' COMMENT 'User who created this variable',
    last_used_at  TIMESTAMP    NULL       COMMENT 'Last time referenced in a run',
    last_used_by  VARCHAR      NULL       COMMENT 'Agent ID that last used this variable',
    created_at    TIMESTAMP    NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMP    NOT NULL DEFAULT NOW()
) COMMENT = 'User-level variables (shared across agents)';
