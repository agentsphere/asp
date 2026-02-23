-- Seed workspace permissions
INSERT INTO permissions (id, name, resource, action, description)
VALUES
    (gen_random_uuid(), 'workspace:read',  'workspace', 'read',  'Read workspace data'),
    (gen_random_uuid(), 'workspace:write', 'workspace', 'write', 'Create/update workspaces'),
    (gen_random_uuid(), 'workspace:admin', 'workspace', 'admin', 'Manage workspace members and settings')
ON CONFLICT (name) DO NOTHING;

-- Grant workspace permissions to admin role
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.name = 'admin'
  AND p.name IN ('workspace:read', 'workspace:write', 'workspace:admin')
ON CONFLICT DO NOTHING;

-- Grant workspace:read and workspace:write to developer role
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.name = 'developer'
  AND p.name IN ('workspace:read', 'workspace:write')
ON CONFLICT DO NOTHING;
