DROP INDEX IF EXISTS idx_artifacts_parent;
DROP INDEX IF EXISTS idx_artifacts_type;

ALTER TABLE artifacts DROP COLUMN IF EXISTS relative_path;
ALTER TABLE artifacts DROP COLUMN IF EXISTS parent_id;
ALTER TABLE artifacts DROP COLUMN IF EXISTS is_directory;
ALTER TABLE artifacts DROP COLUMN IF EXISTS config;
ALTER TABLE artifacts DROP COLUMN IF EXISTS artifact_type;
ALTER TABLE artifacts DROP COLUMN IF EXISTS step_id;
