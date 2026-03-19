-- Backfill columns added after initial schema release.
-- Each statement is idempotent: errors are silently skipped by the migrator
-- when the column already exists.

ALTER TABLE runs ADD COLUMN node_id VARCHAR NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN executor_node_id VARCHAR NULL;
ALTER TABLE tasks ADD COLUMN lease_token VARCHAR NULL;
ALTER TABLE tasks ADD COLUMN lease_node_id VARCHAR NULL;
ALTER TABLE tasks ADD COLUMN lease_expires_at TIMESTAMP NULL;
ALTER TABLE task_history ADD COLUMN executed_by_node_id VARCHAR NULL;
