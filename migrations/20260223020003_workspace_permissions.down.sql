-- Remove workspace role_permissions
DELETE FROM role_permissions
WHERE permission_id IN (
    SELECT id FROM permissions WHERE name IN ('workspace:read', 'workspace:write', 'workspace:admin')
);

-- Remove workspace permissions
DELETE FROM permissions WHERE name IN ('workspace:read', 'workspace:write', 'workspace:admin');
