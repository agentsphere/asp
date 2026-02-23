-- Add workspace_id and environment columns to secrets for hierarchical resolution
ALTER TABLE secrets
    ADD COLUMN workspace_id UUID REFERENCES workspaces(id),
    ADD COLUMN environment  TEXT CHECK (environment IS NULL OR
                            environment IN ('preview', 'staging', 'production'));

-- Replace old unique constraint with hierarchical one
ALTER TABLE secrets DROP CONSTRAINT IF EXISTS secrets_project_id_name_key;

-- New uniqueness: (workspace, project, environment, name)
CREATE UNIQUE INDEX idx_secrets_scoped ON secrets (
    COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000'::uuid),
    COALESCE(project_id,   '00000000-0000-0000-0000-000000000000'::uuid),
    COALESCE(environment,  '__none__'),
    name
);

-- Keep global index working
DROP INDEX IF EXISTS idx_secrets_global_name;
CREATE UNIQUE INDEX idx_secrets_global_name ON secrets(name)
    WHERE project_id IS NULL AND workspace_id IS NULL AND environment IS NULL;
