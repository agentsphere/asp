-- Re-require project_id on agent_sessions (delete any NULL rows first).
DELETE FROM agent_sessions WHERE project_id IS NULL;

ALTER TABLE agent_sessions
    ALTER COLUMN project_id SET NOT NULL;
