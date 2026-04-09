-- ============================================================
-- 1. Denormalize spans: add project_id, session_id, user_id
--    (mirrors log_entries pattern — disk is cheap, CPU on
--    correlated subqueries against millions of rows is not)
-- ============================================================

ALTER TABLE spans ADD COLUMN project_id UUID REFERENCES projects(id);
ALTER TABLE spans ADD COLUMN session_id UUID REFERENCES agent_sessions(id);
ALTER TABLE spans ADD COLUMN user_id UUID REFERENCES users(id);

-- No backfill needed: pre-alpha, no running installations.
-- New spans will be written with project_id/session_id/user_id
-- from the store layer update below.

-- ============================================================
-- 2. Composite indexes for aggregation queries
-- ============================================================

-- Topology: self-join on spans filtering by kind + time range + project
CREATE INDEX idx_spans_project_kind_started
    ON spans(project_id, kind, started_at);

-- Error breakdown: filter by status=error + kind=server + time
CREATE INDEX idx_spans_status_kind_started
    ON spans(status, kind, started_at);

-- Session timeline: spans for a given session
CREATE INDEX idx_spans_session_started
    ON spans(session_id, started_at)
    WHERE session_id IS NOT NULL;

-- Trace aggregation: filter by project + time range
CREATE INDEX idx_traces_project_started
    ON traces(project_id, started_at);

-- Trace aggregation (global): time range only
CREATE INDEX idx_traces_started
    ON traces(started_at);

-- Deploy markers for load timeline
CREATE INDEX idx_deploy_releases_project_started
    ON deploy_releases(project_id, started_at);

-- Alert rules by project (missing)
CREATE INDEX idx_alert_rules_project
    ON alert_rules(project_id);
