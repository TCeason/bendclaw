-- Agent runtime configuration: prompts, behavior, and limits.
CREATE TABLE IF NOT EXISTS agent_config (
    agent_id          VARCHAR   NOT NULL   COMMENT 'Agent identifier (unique per row)',
    system_prompt     TEXT      NOT NULL DEFAULT '' COMMENT 'Persistent system prompt',
    identity          TEXT      NOT NULL DEFAULT '' COMMENT 'Agent identity definition (PromptBuilder layer 1)',
    soul              TEXT      NOT NULL DEFAULT '' COMMENT 'Agent behavior/personality rules (PromptBuilder layer 2)',
    token_limit_total BIGINT UNSIGNED NULL COMMENT 'Total token usage limit',
    token_limit_daily BIGINT UNSIGNED NULL COMMENT 'Daily token usage limit',
    llm_config        VARIANT   NULL COMMENT 'Per-agent LLM configuration (JSON, nullable)',
    updated_at        TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Agent runtime configuration';

-- Agent config version history for rollback and audit.
CREATE TABLE IF NOT EXISTS agent_config_versions (
    id                VARCHAR   NOT NULL   COMMENT 'ULID primary key',
    agent_id          VARCHAR   NOT NULL   COMMENT 'FK -> agent_config.agent_id',
    version           UINT32    NOT NULL   COMMENT 'Monotonically increasing version number',
    label             VARCHAR   NOT NULL DEFAULT '' COMMENT 'Human label (e.g. stable, v1.2)',
    `stage`           VARCHAR   NOT NULL DEFAULT 'published' COMMENT 'draft | published',
    system_prompt     VARCHAR   NOT NULL DEFAULT '' COMMENT 'System prompt snapshot',
    identity          TEXT      NOT NULL DEFAULT '' COMMENT 'Identity snapshot',
    soul              TEXT      NOT NULL DEFAULT '' COMMENT 'Soul snapshot',
    token_limit_total BIGINT UNSIGNED NULL COMMENT 'Total token limit snapshot',
    token_limit_daily BIGINT UNSIGNED NULL COMMENT 'Daily token limit snapshot',
    llm_config        VARIANT   NULL COMMENT 'LLM configuration snapshot (JSON, nullable)',
    notes             VARCHAR   NOT NULL DEFAULT '' COMMENT 'Change notes',
    created_at        TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'Agent config version history';
