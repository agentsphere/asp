-- Allow agent sessions without a project (for create-app flow).
ALTER TABLE agent_sessions
    ALTER COLUMN project_id DROP NOT NULL;
