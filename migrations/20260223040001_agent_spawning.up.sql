-- Add parent session tracking and spawn control to agent_sessions
ALTER TABLE agent_sessions
    ADD COLUMN parent_session_id UUID REFERENCES agent_sessions(id),
    ADD COLUMN spawn_depth       INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN allowed_child_roles TEXT[];

ALTER TABLE agent_sessions
    ADD CONSTRAINT chk_spawn_depth CHECK (spawn_depth <= 5);

CREATE INDEX idx_sessions_parent ON agent_sessions(parent_session_id)
    WHERE parent_session_id IS NOT NULL;
