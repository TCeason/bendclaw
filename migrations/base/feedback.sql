-- User feedback on agent run outputs.
CREATE TABLE IF NOT EXISTS feedback (
    id            VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    agent_id      VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent identifier',
    session_id    VARCHAR   NULL       COMMENT 'FK -> sessions.id',
    run_id        VARCHAR   NULL       COMMENT 'FK -> runs.id',
    user_id       VARCHAR   NOT NULL DEFAULT '' COMMENT 'Owner user',
    scope         VARCHAR   NOT NULL DEFAULT 'shared' COMMENT 'private | shared',
    created_by    VARCHAR   NOT NULL DEFAULT '' COMMENT 'User who submitted this feedback',
    rating        INT       NOT NULL   COMMENT '-1 (thumbs down) or 1 (thumbs up)',
    comment       TEXT      NULL       COMMENT 'Optional user comment',
    created_at    TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'User feedback on agent run outputs';
