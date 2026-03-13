-- Per-step conditions: control when a step runs based on trigger type and branch patterns
ALTER TABLE pipeline_steps ADD COLUMN condition_events TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE pipeline_steps ADD COLUMN condition_branches TEXT[] NOT NULL DEFAULT '{}';

-- Deploy-test step configuration (JSON blob, nullable)
ALTER TABLE pipeline_steps ADD COLUMN deploy_test JSONB;
