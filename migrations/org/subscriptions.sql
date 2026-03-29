-- User opt-in to shared resources from others (lives in evotai_meta).
CREATE TABLE IF NOT EXISTS resource_subscriptions (
    user_id       VARCHAR   NOT NULL COMMENT 'Subscribing user',
    resource_type VARCHAR   NOT NULL COMMENT 'variable | skill',
    resource_key  VARCHAR   NOT NULL COMMENT 'variable.id or skill name',
    owner_id      VARCHAR   NOT NULL COMMENT 'Resource owner user_id',
    revoked       BOOLEAN   NOT NULL DEFAULT false COMMENT 'Soft-revoke subscription',
    created_at    TIMESTAMP NOT NULL DEFAULT NOW()
) COMMENT = 'User opt-in to shared resources from others';

ALTER TABLE resource_subscriptions ADD COLUMN IF NOT EXISTS revoked BOOLEAN NOT NULL DEFAULT false COMMENT 'Soft-revoke subscription';
