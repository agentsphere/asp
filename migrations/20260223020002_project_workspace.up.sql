-- Add optional workspace_id to projects
ALTER TABLE projects
    ADD COLUMN workspace_id UUID REFERENCES workspaces(id);

CREATE INDEX idx_projects_workspace ON projects(workspace_id)
    WHERE workspace_id IS NOT NULL;
