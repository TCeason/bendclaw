-- Global agent registry (lives in evotai_meta database, one per org tenant).
CREATE TABLE IF NOT EXISTS evotai_agents (
    agent_id       VARCHAR   NOT NULL   COMMENT 'Agent identifier (matches per-agent DB suffix)',
    database_name  VARCHAR   NOT NULL   COMMENT 'Full database name (e.g. bendclaw_v2_myagent)',
    display_name   VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent display name',
    description    VARCHAR   NOT NULL DEFAULT '' COMMENT 'Human-readable description',
    model          VARCHAR   NOT NULL DEFAULT '' COMMENT 'LLM model (e.g. gpt-4o, claude-sonnet-4-5)',
    visibility     VARCHAR   NOT NULL DEFAULT 'private' COMMENT 'private | shared | public',
    user_id        VARCHAR   NOT NULL DEFAULT '' COMMENT 'Owner user ID',
    status         VARCHAR   NOT NULL DEFAULT 'active' COMMENT 'active | archived | deleted',
    schema_version VARCHAR   NOT NULL DEFAULT '' COMMENT 'Agent DB schema version for migration tracking',
    last_active_at TIMESTAMP NULL      COMMENT 'Last time this agent was used',
    created_by     VARCHAR   NOT NULL DEFAULT '' COMMENT 'User who created this agent',
    created_at     TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Global agent registry per org';
