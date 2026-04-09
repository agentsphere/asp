ALTER TABLE spans DROP COLUMN IF EXISTS project_id;
ALTER TABLE spans DROP COLUMN IF EXISTS session_id;
ALTER TABLE spans DROP COLUMN IF EXISTS user_id;

DROP INDEX IF EXISTS idx_spans_project_kind_started;
DROP INDEX IF EXISTS idx_spans_status_kind_started;
DROP INDEX IF EXISTS idx_spans_session_started;
DROP INDEX IF EXISTS idx_traces_project_started;
DROP INDEX IF EXISTS idx_traces_started;
DROP INDEX IF EXISTS idx_deploy_releases_project_started;
DROP INDEX IF EXISTS idx_alert_rules_project;
