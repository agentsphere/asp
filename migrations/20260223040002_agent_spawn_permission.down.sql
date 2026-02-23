DELETE FROM role_permissions
WHERE permission_id IN (
    SELECT id FROM permissions WHERE name = 'agent:spawn'
);

DELETE FROM permissions WHERE name = 'agent:spawn';
