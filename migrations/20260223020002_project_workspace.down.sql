DROP INDEX IF EXISTS idx_projects_workspace;
ALTER TABLE projects DROP COLUMN IF EXISTS workspace_id;
