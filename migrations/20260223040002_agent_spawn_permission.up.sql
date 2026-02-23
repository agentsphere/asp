-- Add agent:spawn permission
INSERT INTO permissions (name, resource, action, description)
VALUES ('agent:spawn', 'agent', 'spawn', 'Spawn child agent sessions')
ON CONFLICT (name) DO NOTHING;

-- Grant to admin role
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.name = 'admin' AND p.name = 'agent:spawn'
ON CONFLICT DO NOTHING;

-- Grant to developer role
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.name = 'developer' AND p.name = 'agent:spawn'
ON CONFLICT DO NOTHING;
