-- User-level skills (shared across agents, lives in evotai_meta).
CREATE TABLE IF NOT EXISTS skills (
    name         VARCHAR      NOT NULL   COMMENT 'Unique skill identifier (slug)',
    version      VARCHAR      NOT NULL DEFAULT '0.0.0' COMMENT 'Semver',
    scope        VARCHAR      NOT NULL DEFAULT 'shared' COMMENT 'private | shared',
    source       VARCHAR      NOT NULL DEFAULT 'agent' COMMENT 'local | hub | github | agent',
    user_id      VARCHAR      NOT NULL   COMMENT 'Owner user ID',
    created_by   VARCHAR      NULL       COMMENT 'User who created the skill',
    description  VARCHAR      NOT NULL DEFAULT '' COMMENT 'Human-readable summary',
    timeout      INT UNSIGNED NOT NULL DEFAULT 30 COMMENT 'Execution timeout in seconds',
    executable   BOOLEAN      NOT NULL DEFAULT FALSE COMMENT 'Has a runnable script',
    enabled      BOOLEAN      NOT NULL DEFAULT TRUE COMMENT 'Soft-disable without deleting',
    content      VARCHAR      NOT NULL DEFAULT '' COMMENT 'SKILL.md body (prompt + docs)',
    sha256       VARCHAR      NOT NULL DEFAULT '' COMMENT 'Content checksum for sync',
    last_used_by VARCHAR      NULL       COMMENT 'Agent ID that last used this skill',
    updated_at   TIMESTAMP    NOT NULL DEFAULT NOW()
) COMMENT = 'User-level skills (shared across agents)';

-- Files bundled with a skill.
CREATE TABLE IF NOT EXISTS skill_files (
    skill_name VARCHAR   NOT NULL   COMMENT 'FK -> skills.name',
    user_id    VARCHAR   NOT NULL   COMMENT 'Matches parent skill user_id',
    created_by VARCHAR   NULL       COMMENT 'Matches parent skill created_by',
    file_path  VARCHAR   NOT NULL   COMMENT 'Relative path within skill dir',
    file_body  VARCHAR   NOT NULL DEFAULT '' COMMENT 'File content',
    sha256     VARCHAR   NOT NULL DEFAULT '' COMMENT 'File checksum',
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Files bundled with a skill';
