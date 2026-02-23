-- Drop new indexes
DROP INDEX IF EXISTS idx_secrets_scoped;
DROP INDEX IF EXISTS idx_secrets_global_name;

-- Restore original unique constraint
ALTER TABLE secrets ADD CONSTRAINT secrets_project_id_name_key UNIQUE (project_id, name);

-- Restore original global index
CREATE UNIQUE INDEX idx_secrets_global_name ON secrets(name) WHERE project_id IS NULL;

-- Drop new columns
ALTER TABLE secrets DROP COLUMN IF EXISTS environment;
ALTER TABLE secrets DROP COLUMN IF EXISTS workspace_id;
