-- Seed agent roles (is_system = false so admins can customize their permissions)
INSERT INTO roles (id, name, description, is_system) VALUES
  (gen_random_uuid(), 'agent-dev',     'Agent: developer — code within a project',           false),
  (gen_random_uuid(), 'agent-ops',     'Agent: operations — deploy and observe a project',   false),
  (gen_random_uuid(), 'agent-test',    'Agent: tester — read-only project + observability',  false),
  (gen_random_uuid(), 'agent-review',  'Agent: reviewer — read-only project access',         false),
  (gen_random_uuid(), 'agent-manager', 'Agent: manager — create projects, spawn agents',     false)
ON CONFLICT (name) DO NOTHING;

-- Wire permissions for agent roles
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE (r.name, p.name) IN (
  ('agent-dev', 'project:read'), ('agent-dev', 'project:write'), ('agent-dev', 'secret:read'),
  ('agent-dev', 'registry:pull'), ('agent-dev', 'registry:push'),
  ('agent-ops', 'project:read'), ('agent-ops', 'deploy:read'), ('agent-ops', 'deploy:promote'),
  ('agent-ops', 'observe:read'), ('agent-ops', 'observe:write'), ('agent-ops', 'alert:manage'),
  ('agent-ops', 'secret:read'), ('agent-ops', 'registry:pull'),
  ('agent-test', 'project:read'), ('agent-test', 'observe:read'), ('agent-test', 'registry:pull'),
  ('agent-review', 'project:read'), ('agent-review', 'observe:read'),
  ('agent-manager', 'project:read'), ('agent-manager', 'project:write'),
  ('agent-manager', 'agent:run'), ('agent-manager', 'agent:spawn'),
  ('agent-manager', 'deploy:read'), ('agent-manager', 'observe:read'),
  ('agent-manager', 'workspace:read')
) ON CONFLICT DO NOTHING;

-- Add scope_workspace_id to api_tokens for hard workspace boundary
ALTER TABLE api_tokens
    ADD COLUMN IF NOT EXISTS scope_workspace_id UUID REFERENCES workspaces(id);

-- Indexes for scope lookups
CREATE INDEX IF NOT EXISTS idx_api_tokens_scope_workspace ON api_tokens(scope_workspace_id)
    WHERE scope_workspace_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_api_tokens_scope_project ON api_tokens(project_id)
    WHERE project_id IS NOT NULL;
