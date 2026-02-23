ALTER TABLE agent_sessions DROP CONSTRAINT IF EXISTS chk_spawn_depth;
DROP INDEX IF EXISTS idx_sessions_parent;
ALTER TABLE agent_sessions DROP COLUMN IF EXISTS allowed_child_roles;
ALTER TABLE agent_sessions DROP COLUMN IF EXISTS spawn_depth;
ALTER TABLE agent_sessions DROP COLUMN IF EXISTS parent_session_id;
