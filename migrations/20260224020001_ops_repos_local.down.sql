ALTER TABLE ops_repos ADD COLUMN repo_url TEXT;
UPDATE ops_repos SET repo_url = repo_path;
ALTER TABLE ops_repos ALTER COLUMN repo_url SET NOT NULL;
ALTER TABLE ops_repos DROP COLUMN repo_path;
ALTER TABLE ops_repos ADD COLUMN sync_interval_s INTEGER NOT NULL DEFAULT 60;
