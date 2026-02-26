-- Remove scope indexes
DROP INDEX IF EXISTS idx_api_tokens_scope_project;
DROP INDEX IF EXISTS idx_api_tokens_scope_workspace;

-- Remove scope_workspace_id column
ALTER TABLE api_tokens DROP COLUMN IF EXISTS scope_workspace_id;

-- Remove agent role permissions
DELETE FROM role_permissions WHERE role_id IN (
    SELECT id FROM roles WHERE name IN ('agent-dev', 'agent-ops', 'agent-test', 'agent-review', 'agent-manager')
);

-- Remove agent roles
DELETE FROM roles WHERE name IN ('agent-dev', 'agent-ops', 'agent-test', 'agent-review', 'agent-manager');
