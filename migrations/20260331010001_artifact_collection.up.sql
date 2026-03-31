ALTER TABLE artifacts ADD COLUMN step_id UUID REFERENCES pipeline_steps(id) ON DELETE CASCADE;
ALTER TABLE artifacts ADD COLUMN artifact_type TEXT;
ALTER TABLE artifacts ADD COLUMN config JSONB;
ALTER TABLE artifacts ADD COLUMN is_directory BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE artifacts ADD COLUMN parent_id UUID REFERENCES artifacts(id) ON DELETE CASCADE;
ALTER TABLE artifacts ADD COLUMN relative_path TEXT;

CREATE INDEX idx_artifacts_type ON artifacts(pipeline_id, artifact_type);
CREATE INDEX idx_artifacts_parent ON artifacts(parent_id);
