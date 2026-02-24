-- Migrate ops_repos from remote URL model to local bare repo model.
-- repo_url → repo_path (local filesystem path)
-- Drop sync_interval_s (no remote syncing needed)

ALTER TABLE ops_repos ADD COLUMN repo_path TEXT;

-- Copy repo_url values to repo_path as a migration path
UPDATE ops_repos SET repo_path = repo_url;

ALTER TABLE ops_repos ALTER COLUMN repo_path SET NOT NULL;
ALTER TABLE ops_repos DROP COLUMN repo_url;
ALTER TABLE ops_repos DROP COLUMN sync_interval_s;
