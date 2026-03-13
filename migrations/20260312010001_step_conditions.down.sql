ALTER TABLE pipeline_steps DROP COLUMN IF EXISTS deploy_test;
ALTER TABLE pipeline_steps DROP COLUMN IF EXISTS condition_branches;
ALTER TABLE pipeline_steps DROP COLUMN IF EXISTS condition_events;
