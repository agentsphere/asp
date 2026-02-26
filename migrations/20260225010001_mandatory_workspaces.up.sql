-- PR 2: Make workspace_id NOT NULL on projects.
-- Backfill orphan projects by creating personal workspaces for their owners.

-- 1. Create personal workspace for each user who owns projects without a workspace
INSERT INTO workspaces (id, name, display_name, description, owner_id)
SELECT gen_random_uuid(),
       u.name || '-personal',
       u.display_name || '''s workspace',
       'Auto-created personal workspace',
       u.id
FROM users u
WHERE u.user_type = 'human'
  AND u.is_active = true
  AND NOT EXISTS (
    SELECT 1 FROM workspaces w WHERE w.owner_id = u.id AND w.is_active = true
  )
  AND EXISTS (
    SELECT 1 FROM projects p WHERE p.owner_id = u.id AND p.workspace_id IS NULL AND p.is_active = true
  );

-- 2. Add workspace owners as members (if not already)
INSERT INTO workspace_members (id, workspace_id, user_id, role)
SELECT gen_random_uuid(), w.id, w.owner_id, 'owner'
FROM workspaces w
WHERE NOT EXISTS (
    SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = w.id AND wm.user_id = w.owner_id
);

-- 3. Assign orphan projects to their owner's workspace
UPDATE projects p
SET workspace_id = (
    SELECT w.id FROM workspaces w
    WHERE w.owner_id = p.owner_id AND w.is_active = true
    ORDER BY w.created_at LIMIT 1
)
WHERE p.workspace_id IS NULL AND p.is_active = true;

-- 4. Make workspace_id NOT NULL
ALTER TABLE projects ALTER COLUMN workspace_id SET NOT NULL;
