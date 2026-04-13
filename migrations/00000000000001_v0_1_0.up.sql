-- v0.1.0: Consolidated schema from 79 individual migrations
-- Generated from pg_dump of platform_dev on 2026-04-13


CREATE FUNCTION set_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$;

CREATE TABLE agent_messages (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    session_id uuid NOT NULL,
    role text NOT NULL,
    content text NOT NULL,
    metadata jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT agent_messages_role_check CHECK ((role = ANY (ARRAY['user'::text, 'assistant'::text, 'system'::text, 'tool'::text, 'text'::text, 'thinking'::text, 'tool_call'::text, 'tool_result'::text, 'milestone'::text, 'error'::text, 'completed'::text, 'waiting_for_input'::text, 'progress_update'::text, 'iframe_available'::text, 'iframe_removed'::text, 'secret_request'::text, 'unknown'::text])))
);

CREATE TABLE agent_sessions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid,
    user_id uuid NOT NULL,
    agent_user_id uuid,
    prompt text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    branch text,
    pod_name text,
    provider text DEFAULT 'claude-code'::text NOT NULL,
    provider_config jsonb,
    cost_tokens bigint,
    cost_usd numeric(10,4),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    parent_session_id uuid,
    spawn_depth integer DEFAULT 0 NOT NULL,
    allowed_child_roles text[],
    execution_mode text DEFAULT 'pod'::text NOT NULL,
    uses_pubsub boolean DEFAULT false NOT NULL,
    session_namespace text,
    CONSTRAINT agent_sessions_execution_mode_check CHECK ((execution_mode = ANY (ARRAY['pod'::text, 'cli_subprocess'::text, 'manager'::text]))),
    CONSTRAINT agent_sessions_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'running'::text, 'completed'::text, 'failed'::text, 'stopped'::text]))),
    CONSTRAINT chk_spawn_depth CHECK ((spawn_depth <= 5))
);

CREATE TABLE alert_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    rule_id uuid NOT NULL,
    status text NOT NULL,
    value double precision,
    message text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    resolved_at timestamp with time zone,
    CONSTRAINT alert_events_status_check CHECK ((status = ANY (ARRAY['firing'::text, 'resolved'::text])))
);

CREATE TABLE alert_rules (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    description text,
    query text NOT NULL,
    condition text NOT NULL,
    threshold double precision,
    for_seconds integer DEFAULT 60 NOT NULL,
    severity text DEFAULT 'warning'::text NOT NULL,
    notify_channels text[] DEFAULT '{}'::text[] NOT NULL,
    project_id uuid,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT alert_rules_severity_check CHECK ((severity = ANY (ARRAY['info'::text, 'warning'::text, 'critical'::text])))
);

CREATE TABLE api_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    name text NOT NULL,
    token_hash text NOT NULL,
    scopes text[] DEFAULT '{}'::text[] NOT NULL,
    project_id uuid,
    last_used_at timestamp with time zone,
    expires_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    scope_workspace_id uuid,
    registry_tag_pattern text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE artifacts (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    pipeline_id uuid NOT NULL,
    name text NOT NULL,
    minio_path text NOT NULL,
    content_type text,
    size_bytes bigint,
    expires_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    step_id uuid,
    artifact_type text,
    config jsonb,
    is_directory boolean DEFAULT false NOT NULL,
    parent_id uuid,
    relative_path text
);

CREATE TABLE audit_log (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    actor_id uuid NOT NULL,
    actor_name text NOT NULL,
    action text NOT NULL,
    resource text NOT NULL,
    resource_id uuid,
    project_id uuid,
    detail jsonb,
    ip_addr inet,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE auth_sessions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    token_hash text NOT NULL,
    ip_addr inet,
    user_agent text,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE branch_protection_rules (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    pattern text NOT NULL,
    require_pr boolean DEFAULT true NOT NULL,
    block_force_push boolean DEFAULT true NOT NULL,
    required_approvals integer DEFAULT 0 NOT NULL,
    dismiss_stale_reviews boolean DEFAULT true NOT NULL,
    required_checks text[] DEFAULT '{}'::text[] NOT NULL,
    require_up_to_date boolean DEFAULT false NOT NULL,
    allow_admin_bypass boolean DEFAULT false NOT NULL,
    merge_methods text[] DEFAULT '{merge}'::text[] NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT branch_protection_rules_required_approvals_check CHECK ((required_approvals >= 0))
);

CREATE TABLE cli_credentials (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    auth_type text NOT NULL,
    encrypted_data bytea NOT NULL,
    token_expires_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT cli_credentials_auth_type_check CHECK ((auth_type = ANY (ARRAY['oauth'::text, 'setup_token'::text])))
);

CREATE TABLE comments (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    issue_id uuid,
    mr_id uuid,
    author_id uuid NOT NULL,
    body text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT comments_check CHECK (((issue_id IS NOT NULL) OR (mr_id IS NOT NULL)))
);

CREATE TABLE delegations (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    delegator_id uuid NOT NULL,
    delegate_id uuid NOT NULL,
    permission_id uuid NOT NULL,
    project_id uuid,
    expires_at timestamp with time zone,
    reason text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    revoked_at timestamp with time zone
);

CREATE TABLE deploy_releases (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    target_id uuid NOT NULL,
    project_id uuid NOT NULL,
    image_ref text NOT NULL,
    commit_sha text,
    strategy text DEFAULT 'rolling'::text NOT NULL,
    phase text DEFAULT 'pending'::text NOT NULL,
    traffic_weight integer DEFAULT 0 NOT NULL,
    health text DEFAULT 'unknown'::text NOT NULL,
    current_step integer DEFAULT 0 NOT NULL,
    rollout_config jsonb DEFAULT '{}'::jsonb NOT NULL,
    analysis_config jsonb,
    values_override jsonb,
    tracked_resources jsonb DEFAULT '[]'::jsonb NOT NULL,
    deployed_by uuid,
    pipeline_id uuid,
    started_at timestamp with time zone,
    completed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT deploy_releases_health_check CHECK ((health = ANY (ARRAY['unknown'::text, 'healthy'::text, 'degraded'::text, 'unhealthy'::text]))),
    CONSTRAINT deploy_releases_phase_check CHECK ((phase = ANY (ARRAY['pending'::text, 'progressing'::text, 'holding'::text, 'paused'::text, 'promoting'::text, 'completed'::text, 'rolling_back'::text, 'rolled_back'::text, 'cancelled'::text, 'failed'::text]))),
    CONSTRAINT deploy_releases_strategy_check CHECK ((strategy = ANY (ARRAY['rolling'::text, 'canary'::text, 'ab_test'::text]))),
    CONSTRAINT deploy_releases_traffic_weight_check CHECK (((traffic_weight >= 0) AND (traffic_weight <= 100)))
);

CREATE TABLE deploy_targets (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    name text NOT NULL,
    environment text DEFAULT 'production'::text NOT NULL,
    branch text,
    branch_slug text,
    ttl_hours integer,
    expires_at timestamp with time zone,
    default_strategy text DEFAULT 'rolling'::text NOT NULL,
    ops_repo_id uuid,
    manifest_path text,
    is_active boolean DEFAULT true NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    hostname text,
    CONSTRAINT deploy_targets_default_strategy_check CHECK ((default_strategy = ANY (ARRAY['rolling'::text, 'canary'::text, 'ab_test'::text]))),
    CONSTRAINT deploy_targets_environment_check CHECK ((environment = ANY (ARRAY['preview'::text, 'staging'::text, 'production'::text])))
);

CREATE TABLE feature_flag_history (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    flag_id uuid NOT NULL,
    action text NOT NULL,
    actor_id uuid,
    previous_value jsonb,
    new_value jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT feature_flag_history_action_check CHECK ((action = ANY (ARRAY['created'::text, 'updated'::text, 'toggled'::text, 'deleted'::text, 'rule_added'::text, 'rule_updated'::text, 'rule_deleted'::text, 'override_set'::text, 'override_deleted'::text])))
);

CREATE TABLE feature_flag_overrides (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    flag_id uuid NOT NULL,
    user_id uuid NOT NULL,
    serve_value jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE feature_flag_rules (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    flag_id uuid NOT NULL,
    priority integer DEFAULT 0 NOT NULL,
    rule_type text NOT NULL,
    attribute_name text,
    attribute_values text[] DEFAULT '{}'::text[] NOT NULL,
    percentage integer,
    serve_value jsonb NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT feature_flag_rules_percentage_check CHECK (((percentage IS NULL) OR ((percentage >= 0) AND (percentage <= 100)))),
    CONSTRAINT feature_flag_rules_rule_type_check CHECK ((rule_type = ANY (ARRAY['user_id'::text, 'user_attribute'::text, 'percentage'::text])))
);

CREATE TABLE feature_flags (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid,
    key text NOT NULL,
    flag_type text DEFAULT 'boolean'::text NOT NULL,
    default_value jsonb DEFAULT 'false'::jsonb NOT NULL,
    environment text,
    enabled boolean DEFAULT false NOT NULL,
    description text,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT feature_flags_environment_check CHECK (((environment IS NULL) OR (environment = ANY (ARRAY['staging'::text, 'production'::text])))),
    CONSTRAINT feature_flags_flag_type_check CHECK ((flag_type = ANY (ARRAY['boolean'::text, 'percentage'::text, 'variant'::text, 'json'::text])))
);

CREATE TABLE issues (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    number integer NOT NULL,
    author_id uuid NOT NULL,
    title text NOT NULL,
    body text,
    status text DEFAULT 'open'::text NOT NULL,
    labels text[] DEFAULT '{}'::text[] NOT NULL,
    assignee_id uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT issues_status_check CHECK ((status = ANY (ARRAY['open'::text, 'closed'::text])))
);

CREATE TABLE llm_provider_configs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    provider_type text NOT NULL,
    label text DEFAULT ''::text NOT NULL,
    encrypted_config bytea NOT NULL,
    model text,
    validation_status text DEFAULT 'untested'::text NOT NULL,
    last_validated_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT llm_provider_configs_provider_type_check CHECK ((provider_type = ANY (ARRAY['bedrock'::text, 'vertex'::text, 'azure_foundry'::text, 'custom_endpoint'::text]))),
    CONSTRAINT llm_provider_configs_validation_status_check CHECK ((validation_status = ANY (ARRAY['untested'::text, 'valid'::text, 'invalid'::text])))
);

CREATE TABLE log_entries (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    "timestamp" timestamp with time zone DEFAULT now() NOT NULL,
    trace_id text,
    span_id text,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    service text NOT NULL,
    level text DEFAULT 'info'::text NOT NULL,
    message text NOT NULL,
    attributes jsonb,
    namespace text,
    pod text,
    container text,
    source text DEFAULT 'external'::text NOT NULL,
    CONSTRAINT log_entries_level_check CHECK ((level = ANY (ARRAY['trace'::text, 'debug'::text, 'info'::text, 'warn'::text, 'error'::text, 'fatal'::text]))),
    CONSTRAINT log_entries_source_check CHECK ((source = ANY (ARRAY['system'::text, 'api'::text, 'session'::text, 'external'::text])))
)
PARTITION BY RANGE ("timestamp");

CREATE TABLE log_entries_p_202604 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    "timestamp" timestamp with time zone DEFAULT now() NOT NULL,
    trace_id text,
    span_id text,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    service text NOT NULL,
    level text DEFAULT 'info'::text NOT NULL,
    message text NOT NULL,
    attributes jsonb,
    namespace text,
    pod text,
    container text,
    source text DEFAULT 'external'::text NOT NULL,
    CONSTRAINT log_entries_level_check CHECK ((level = ANY (ARRAY['trace'::text, 'debug'::text, 'info'::text, 'warn'::text, 'error'::text, 'fatal'::text]))),
    CONSTRAINT log_entries_source_check CHECK ((source = ANY (ARRAY['system'::text, 'api'::text, 'session'::text, 'external'::text])))
);

CREATE TABLE log_entries_p_202605 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    "timestamp" timestamp with time zone DEFAULT now() NOT NULL,
    trace_id text,
    span_id text,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    service text NOT NULL,
    level text DEFAULT 'info'::text NOT NULL,
    message text NOT NULL,
    attributes jsonb,
    namespace text,
    pod text,
    container text,
    source text DEFAULT 'external'::text NOT NULL,
    CONSTRAINT log_entries_level_check CHECK ((level = ANY (ARRAY['trace'::text, 'debug'::text, 'info'::text, 'warn'::text, 'error'::text, 'fatal'::text]))),
    CONSTRAINT log_entries_source_check CHECK ((source = ANY (ARRAY['system'::text, 'api'::text, 'session'::text, 'external'::text])))
);

CREATE TABLE log_entries_p_202606 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    "timestamp" timestamp with time zone DEFAULT now() NOT NULL,
    trace_id text,
    span_id text,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    service text NOT NULL,
    level text DEFAULT 'info'::text NOT NULL,
    message text NOT NULL,
    attributes jsonb,
    namespace text,
    pod text,
    container text,
    source text DEFAULT 'external'::text NOT NULL,
    CONSTRAINT log_entries_level_check CHECK ((level = ANY (ARRAY['trace'::text, 'debug'::text, 'info'::text, 'warn'::text, 'error'::text, 'fatal'::text]))),
    CONSTRAINT log_entries_source_check CHECK ((source = ANY (ARRAY['system'::text, 'api'::text, 'session'::text, 'external'::text])))
);

CREATE TABLE log_entries_p_202607 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    "timestamp" timestamp with time zone DEFAULT now() NOT NULL,
    trace_id text,
    span_id text,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    service text NOT NULL,
    level text DEFAULT 'info'::text NOT NULL,
    message text NOT NULL,
    attributes jsonb,
    namespace text,
    pod text,
    container text,
    source text DEFAULT 'external'::text NOT NULL,
    CONSTRAINT log_entries_level_check CHECK ((level = ANY (ARRAY['trace'::text, 'debug'::text, 'info'::text, 'warn'::text, 'error'::text, 'fatal'::text]))),
    CONSTRAINT log_entries_source_check CHECK ((source = ANY (ARRAY['system'::text, 'api'::text, 'session'::text, 'external'::text])))
);

CREATE TABLE log_entries_p_202608 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    "timestamp" timestamp with time zone DEFAULT now() NOT NULL,
    trace_id text,
    span_id text,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    service text NOT NULL,
    level text DEFAULT 'info'::text NOT NULL,
    message text NOT NULL,
    attributes jsonb,
    namespace text,
    pod text,
    container text,
    source text DEFAULT 'external'::text NOT NULL,
    CONSTRAINT log_entries_level_check CHECK ((level = ANY (ARRAY['trace'::text, 'debug'::text, 'info'::text, 'warn'::text, 'error'::text, 'fatal'::text]))),
    CONSTRAINT log_entries_source_check CHECK ((source = ANY (ARRAY['system'::text, 'api'::text, 'session'::text, 'external'::text])))
);

CREATE TABLE log_entries_p_202609 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    "timestamp" timestamp with time zone DEFAULT now() NOT NULL,
    trace_id text,
    span_id text,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    service text NOT NULL,
    level text DEFAULT 'info'::text NOT NULL,
    message text NOT NULL,
    attributes jsonb,
    namespace text,
    pod text,
    container text,
    source text DEFAULT 'external'::text NOT NULL,
    CONSTRAINT log_entries_level_check CHECK ((level = ANY (ARRAY['trace'::text, 'debug'::text, 'info'::text, 'warn'::text, 'error'::text, 'fatal'::text]))),
    CONSTRAINT log_entries_source_check CHECK ((source = ANY (ARRAY['system'::text, 'api'::text, 'session'::text, 'external'::text])))
);

CREATE TABLE log_entries_p_hist (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    "timestamp" timestamp with time zone DEFAULT now() NOT NULL,
    trace_id text,
    span_id text,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    service text NOT NULL,
    level text DEFAULT 'info'::text NOT NULL,
    message text NOT NULL,
    attributes jsonb,
    namespace text,
    pod text,
    container text,
    source text DEFAULT 'external'::text NOT NULL,
    CONSTRAINT log_entries_level_check CHECK ((level = ANY (ARRAY['trace'::text, 'debug'::text, 'info'::text, 'warn'::text, 'error'::text, 'fatal'::text]))),
    CONSTRAINT log_entries_source_check CHECK ((source = ANY (ARRAY['system'::text, 'api'::text, 'session'::text, 'external'::text])))
);

CREATE TABLE merge_requests (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    number integer NOT NULL,
    author_id uuid NOT NULL,
    source_branch text NOT NULL,
    target_branch text NOT NULL,
    title text NOT NULL,
    body text,
    status text DEFAULT 'open'::text NOT NULL,
    merged_by uuid,
    merged_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    head_sha text,
    auto_merge boolean DEFAULT false NOT NULL,
    auto_merge_by uuid,
    auto_merge_method text,
    CONSTRAINT merge_requests_status_check CHECK ((status = ANY (ARRAY['open'::text, 'merged'::text, 'closed'::text])))
);

CREATE TABLE mesh_ca (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    root_cert_pem text NOT NULL,
    secret_name text NOT NULL,
    serial_counter bigint DEFAULT 1 NOT NULL,
    not_after timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE mesh_certs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    ca_id uuid NOT NULL,
    spiffe_id text NOT NULL,
    serial bigint NOT NULL,
    not_before timestamp with time zone NOT NULL,
    not_after timestamp with time zone NOT NULL,
    namespace text NOT NULL,
    service text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE metric_samples (
    series_id uuid NOT NULL,
    "timestamp" timestamp with time zone NOT NULL,
    value double precision NOT NULL
)
PARTITION BY RANGE ("timestamp");

CREATE TABLE metric_samples_p_202604 (
    series_id uuid NOT NULL,
    "timestamp" timestamp with time zone NOT NULL,
    value double precision NOT NULL
);

CREATE TABLE metric_samples_p_202605 (
    series_id uuid NOT NULL,
    "timestamp" timestamp with time zone NOT NULL,
    value double precision NOT NULL
);

CREATE TABLE metric_samples_p_202606 (
    series_id uuid NOT NULL,
    "timestamp" timestamp with time zone NOT NULL,
    value double precision NOT NULL
);

CREATE TABLE metric_samples_p_202607 (
    series_id uuid NOT NULL,
    "timestamp" timestamp with time zone NOT NULL,
    value double precision NOT NULL
);

CREATE TABLE metric_samples_p_202608 (
    series_id uuid NOT NULL,
    "timestamp" timestamp with time zone NOT NULL,
    value double precision NOT NULL
);

CREATE TABLE metric_samples_p_202609 (
    series_id uuid NOT NULL,
    "timestamp" timestamp with time zone NOT NULL,
    value double precision NOT NULL
);

CREATE TABLE metric_samples_p_hist (
    series_id uuid NOT NULL,
    "timestamp" timestamp with time zone NOT NULL,
    value double precision NOT NULL
);

CREATE TABLE metric_series (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    labels jsonb DEFAULT '{}'::jsonb NOT NULL,
    metric_type text DEFAULT 'gauge'::text NOT NULL,
    unit text,
    project_id uuid,
    last_value double precision,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT metric_series_metric_type_check CHECK ((metric_type = ANY (ARRAY['gauge'::text, 'counter'::text, 'histogram'::text, 'summary'::text])))
);

CREATE TABLE mr_reviews (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    mr_id uuid NOT NULL,
    reviewer_id uuid NOT NULL,
    verdict text NOT NULL,
    body text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    is_stale boolean DEFAULT false NOT NULL,
    CONSTRAINT mr_reviews_verdict_check CHECK ((verdict = ANY (ARRAY['approve'::text, 'request_changes'::text, 'comment'::text])))
);

CREATE TABLE notifications (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    notification_type text NOT NULL,
    subject text NOT NULL,
    body text,
    channel text DEFAULT 'in_app'::text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    ref_type text,
    ref_id uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT notifications_channel_check CHECK ((channel = ANY (ARRAY['in_app'::text, 'email'::text, 'webhook'::text]))),
    CONSTRAINT notifications_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'sent'::text, 'read'::text, 'failed'::text])))
);

CREATE TABLE ops_repos (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    branch text DEFAULT 'main'::text NOT NULL,
    path text DEFAULT '/'::text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    repo_path text NOT NULL,
    project_id uuid
);

CREATE TABLE passkey_credentials (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    credential_id bytea NOT NULL,
    public_key bytea NOT NULL,
    sign_count bigint DEFAULT 0 NOT NULL,
    discoverable boolean DEFAULT true NOT NULL,
    transports text[] DEFAULT '{}'::text[] NOT NULL,
    name text NOT NULL,
    attestation bytea,
    backup_eligible boolean DEFAULT false NOT NULL,
    backup_state boolean DEFAULT false NOT NULL,
    last_used_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE permissions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    resource text NOT NULL,
    action text NOT NULL,
    description text
);

CREATE TABLE pipeline_steps (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    pipeline_id uuid NOT NULL,
    project_id uuid NOT NULL,
    step_order integer NOT NULL,
    name text NOT NULL,
    image text NOT NULL,
    commands text[] DEFAULT '{}'::text[] NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    log_ref text,
    exit_code integer,
    duration_ms integer,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    condition_events text[] DEFAULT '{}'::text[] NOT NULL,
    condition_branches text[] DEFAULT '{}'::text[] NOT NULL,
    deploy_test jsonb,
    depends_on text[] DEFAULT '{}'::text[] NOT NULL,
    environment jsonb,
    gate boolean DEFAULT false NOT NULL,
    step_type text DEFAULT 'command'::text NOT NULL,
    step_config jsonb,
    started_at timestamp with time zone,
    finished_at timestamp with time zone,
    CONSTRAINT pipeline_steps_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'running'::text, 'success'::text, 'failure'::text, 'skipped'::text]))),
    CONSTRAINT pipeline_steps_step_type_check CHECK ((step_type = ANY (ARRAY['command'::text, 'imagebuild'::text, 'deploy_test'::text, 'gitops_sync'::text, 'deploy_watch'::text])))
);

CREATE TABLE pipelines (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    trigger text NOT NULL,
    git_ref text NOT NULL,
    commit_sha text,
    status text DEFAULT 'pending'::text NOT NULL,
    triggered_by uuid,
    started_at timestamp with time zone,
    finished_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    version text,
    CONSTRAINT pipelines_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'running'::text, 'success'::text, 'failure'::text, 'cancelled'::text]))),
    CONSTRAINT pipelines_trigger_check CHECK ((trigger = ANY (ARRAY['push'::text, 'api'::text, 'schedule'::text, 'mr'::text, 'tag'::text])))
);

CREATE TABLE platform_commands (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid,
    name text NOT NULL,
    description text DEFAULT ''::text NOT NULL,
    prompt_template text NOT NULL,
    persistent_session boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id uuid,
    CONSTRAINT chk_commands_scope CHECK ((((workspace_id IS NULL) AND (project_id IS NULL)) OR ((workspace_id IS NOT NULL) AND (project_id IS NULL)) OR (project_id IS NOT NULL)))
);

CREATE TABLE platform_settings (
    key text NOT NULL,
    value jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE projects (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    owner_id uuid NOT NULL,
    name text NOT NULL,
    display_name text,
    description text,
    visibility text DEFAULT 'private'::text NOT NULL,
    default_branch text DEFAULT 'main'::text NOT NULL,
    repo_path text,
    is_active boolean DEFAULT true NOT NULL,
    next_issue_number integer DEFAULT 0 NOT NULL,
    next_mr_number integer DEFAULT 0 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    agent_image text,
    workspace_id uuid NOT NULL,
    namespace_slug text NOT NULL,
    include_staging boolean DEFAULT false NOT NULL,
    CONSTRAINT projects_visibility_check CHECK ((visibility = ANY (ARRAY['private'::text, 'internal'::text, 'public'::text])))
);

CREATE TABLE registry_blob_links (
    repository_id uuid NOT NULL,
    blob_digest text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE registry_blobs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    digest text NOT NULL,
    size_bytes bigint NOT NULL,
    minio_path text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE registry_manifests (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    repository_id uuid NOT NULL,
    digest text NOT NULL,
    media_type text NOT NULL,
    content bytea NOT NULL,
    size_bytes bigint NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE registry_repositories (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid,
    name text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE registry_tags (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    repository_id uuid NOT NULL,
    name text NOT NULL,
    manifest_digest text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE release_assets (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    release_id uuid NOT NULL,
    name text NOT NULL,
    minio_path text NOT NULL,
    content_type text,
    size_bytes bigint,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE release_history (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    release_id uuid NOT NULL,
    target_id uuid NOT NULL,
    action text NOT NULL,
    phase text NOT NULL,
    traffic_weight integer,
    image_ref text NOT NULL,
    detail jsonb,
    actor_id uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT release_history_action_check CHECK ((action = ANY (ARRAY['created'::text, 'step_advanced'::text, 'analysis_started'::text, 'analysis_completed'::text, 'promoted'::text, 'paused'::text, 'resumed'::text, 'rolled_back'::text, 'cancelled'::text, 'failed'::text, 'health_changed'::text, 'traffic_shifted'::text])))
);

CREATE TABLE releases (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    tag_name text NOT NULL,
    name text NOT NULL,
    body text,
    is_draft boolean DEFAULT false NOT NULL,
    is_prerelease boolean DEFAULT false NOT NULL,
    created_by uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE role_permissions (
    role_id uuid NOT NULL,
    permission_id uuid NOT NULL
);

CREATE TABLE roles (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    description text,
    is_system boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE rollout_analyses (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    release_id uuid NOT NULL,
    step_index integer NOT NULL,
    config jsonb NOT NULL,
    verdict text DEFAULT 'running'::text NOT NULL,
    metric_results jsonb,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    completed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT rollout_analyses_verdict_check CHECK ((verdict = ANY (ARRAY['running'::text, 'pass'::text, 'fail'::text, 'inconclusive'::text, 'cancelled'::text])))
);

CREATE TABLE secrets (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid,
    name text NOT NULL,
    encrypted_value bytea NOT NULL,
    scope text DEFAULT 'pipeline'::text NOT NULL,
    version integer DEFAULT 1 NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id uuid,
    environment text,
    CONSTRAINT secrets_environment_check CHECK (((environment IS NULL) OR (environment = ANY (ARRAY['preview'::text, 'staging'::text, 'production'::text])))),
    CONSTRAINT secrets_scope_check CHECK ((scope = ANY (ARRAY['all'::text, 'pipeline'::text, 'agent'::text, 'test'::text, 'staging'::text, 'prod'::text])))
);

CREATE TABLE setup_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    token_hash text NOT NULL,
    used_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL
);

CREATE TABLE spans (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    name text NOT NULL,
    service text NOT NULL,
    kind text DEFAULT 'internal'::text NOT NULL,
    status text DEFAULT 'ok'::text NOT NULL,
    attributes jsonb,
    events jsonb,
    duration_ms integer,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    CONSTRAINT spans_kind_check CHECK ((kind = ANY (ARRAY['internal'::text, 'server'::text, 'client'::text, 'producer'::text, 'consumer'::text]))),
    CONSTRAINT spans_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'error'::text, 'unset'::text])))
)
PARTITION BY RANGE (started_at);

CREATE TABLE spans_p_202604 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    name text NOT NULL,
    service text NOT NULL,
    kind text DEFAULT 'internal'::text NOT NULL,
    status text DEFAULT 'ok'::text NOT NULL,
    attributes jsonb,
    events jsonb,
    duration_ms integer,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    CONSTRAINT spans_kind_check CHECK ((kind = ANY (ARRAY['internal'::text, 'server'::text, 'client'::text, 'producer'::text, 'consumer'::text]))),
    CONSTRAINT spans_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'error'::text, 'unset'::text])))
);

CREATE TABLE spans_p_202605 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    name text NOT NULL,
    service text NOT NULL,
    kind text DEFAULT 'internal'::text NOT NULL,
    status text DEFAULT 'ok'::text NOT NULL,
    attributes jsonb,
    events jsonb,
    duration_ms integer,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    CONSTRAINT spans_kind_check CHECK ((kind = ANY (ARRAY['internal'::text, 'server'::text, 'client'::text, 'producer'::text, 'consumer'::text]))),
    CONSTRAINT spans_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'error'::text, 'unset'::text])))
);

CREATE TABLE spans_p_202606 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    name text NOT NULL,
    service text NOT NULL,
    kind text DEFAULT 'internal'::text NOT NULL,
    status text DEFAULT 'ok'::text NOT NULL,
    attributes jsonb,
    events jsonb,
    duration_ms integer,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    CONSTRAINT spans_kind_check CHECK ((kind = ANY (ARRAY['internal'::text, 'server'::text, 'client'::text, 'producer'::text, 'consumer'::text]))),
    CONSTRAINT spans_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'error'::text, 'unset'::text])))
);

CREATE TABLE spans_p_202607 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    name text NOT NULL,
    service text NOT NULL,
    kind text DEFAULT 'internal'::text NOT NULL,
    status text DEFAULT 'ok'::text NOT NULL,
    attributes jsonb,
    events jsonb,
    duration_ms integer,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    CONSTRAINT spans_kind_check CHECK ((kind = ANY (ARRAY['internal'::text, 'server'::text, 'client'::text, 'producer'::text, 'consumer'::text]))),
    CONSTRAINT spans_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'error'::text, 'unset'::text])))
);

CREATE TABLE spans_p_202608 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    name text NOT NULL,
    service text NOT NULL,
    kind text DEFAULT 'internal'::text NOT NULL,
    status text DEFAULT 'ok'::text NOT NULL,
    attributes jsonb,
    events jsonb,
    duration_ms integer,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    CONSTRAINT spans_kind_check CHECK ((kind = ANY (ARRAY['internal'::text, 'server'::text, 'client'::text, 'producer'::text, 'consumer'::text]))),
    CONSTRAINT spans_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'error'::text, 'unset'::text])))
);

CREATE TABLE spans_p_202609 (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    name text NOT NULL,
    service text NOT NULL,
    kind text DEFAULT 'internal'::text NOT NULL,
    status text DEFAULT 'ok'::text NOT NULL,
    attributes jsonb,
    events jsonb,
    duration_ms integer,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    CONSTRAINT spans_kind_check CHECK ((kind = ANY (ARRAY['internal'::text, 'server'::text, 'client'::text, 'producer'::text, 'consumer'::text]))),
    CONSTRAINT spans_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'error'::text, 'unset'::text])))
);

CREATE TABLE spans_p_hist (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    name text NOT NULL,
    service text NOT NULL,
    kind text DEFAULT 'internal'::text NOT NULL,
    status text DEFAULT 'ok'::text NOT NULL,
    attributes jsonb,
    events jsonb,
    duration_ms integer,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    CONSTRAINT spans_kind_check CHECK ((kind = ANY (ARRAY['internal'::text, 'server'::text, 'client'::text, 'producer'::text, 'consumer'::text]))),
    CONSTRAINT spans_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'error'::text, 'unset'::text])))
);

CREATE TABLE traces (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    trace_id text NOT NULL,
    project_id uuid,
    session_id uuid,
    user_id uuid,
    root_span text NOT NULL,
    service text NOT NULL,
    status text DEFAULT 'ok'::text NOT NULL,
    duration_ms integer,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT traces_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'error'::text, 'unset'::text])))
);

CREATE TABLE user_gpg_keys (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    key_id text NOT NULL,
    fingerprint text NOT NULL,
    public_key_armor text NOT NULL,
    public_key_bytes bytea NOT NULL,
    emails text[] DEFAULT '{}'::text[] NOT NULL,
    expires_at timestamp with time zone,
    can_sign boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE user_provider_keys (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    provider text DEFAULT 'anthropic'::text NOT NULL,
    encrypted_key bytea NOT NULL,
    key_suffix text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE user_roles (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    role_id uuid NOT NULL,
    project_id uuid,
    granted_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE user_ssh_keys (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    name text NOT NULL,
    algorithm text NOT NULL,
    fingerprint text NOT NULL,
    public_key_openssh text NOT NULL,
    last_used_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE users (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    display_name text,
    email text NOT NULL,
    password_hash text NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    user_type text DEFAULT 'human'::text NOT NULL,
    metadata jsonb,
    active_llm_provider text DEFAULT 'auto'::text NOT NULL,
    CONSTRAINT chk_users_user_type CHECK ((user_type = ANY (ARRAY['human'::text, 'agent'::text, 'service_account'::text])))
);

CREATE TABLE webhooks (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    url text NOT NULL,
    events text[] DEFAULT '{}'::text[] NOT NULL,
    secret text,
    active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE workspace_members (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid NOT NULL,
    user_id uuid NOT NULL,
    role text DEFAULT 'member'::text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_members_role_check CHECK ((role = ANY (ARRAY['owner'::text, 'admin'::text, 'member'::text])))
);

CREATE TABLE workspaces (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    display_name text,
    description text,
    owner_id uuid NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

ALTER TABLE ONLY log_entries ATTACH PARTITION log_entries_p_202604 FOR VALUES FROM ('2026-04-01 00:00:00+00') TO ('2026-05-01 00:00:00+00');

ALTER TABLE ONLY log_entries ATTACH PARTITION log_entries_p_202605 FOR VALUES FROM ('2026-05-01 00:00:00+00') TO ('2026-06-01 00:00:00+00');

ALTER TABLE ONLY log_entries ATTACH PARTITION log_entries_p_202606 FOR VALUES FROM ('2026-06-01 00:00:00+00') TO ('2026-07-01 00:00:00+00');

ALTER TABLE ONLY log_entries ATTACH PARTITION log_entries_p_202607 FOR VALUES FROM ('2026-07-01 00:00:00+00') TO ('2026-08-01 00:00:00+00');

ALTER TABLE ONLY log_entries ATTACH PARTITION log_entries_p_202608 FOR VALUES FROM ('2026-08-01 00:00:00+00') TO ('2026-09-01 00:00:00+00');

ALTER TABLE ONLY log_entries ATTACH PARTITION log_entries_p_202609 FOR VALUES FROM ('2026-09-01 00:00:00+00') TO ('2026-10-01 00:00:00+00');

ALTER TABLE ONLY log_entries ATTACH PARTITION log_entries_p_hist FOR VALUES FROM (MINVALUE) TO ('2026-04-01 00:00:00+00');

ALTER TABLE ONLY metric_samples ATTACH PARTITION metric_samples_p_202604 FOR VALUES FROM ('2026-04-01 00:00:00+00') TO ('2026-05-01 00:00:00+00');

ALTER TABLE ONLY metric_samples ATTACH PARTITION metric_samples_p_202605 FOR VALUES FROM ('2026-05-01 00:00:00+00') TO ('2026-06-01 00:00:00+00');

ALTER TABLE ONLY metric_samples ATTACH PARTITION metric_samples_p_202606 FOR VALUES FROM ('2026-06-01 00:00:00+00') TO ('2026-07-01 00:00:00+00');

ALTER TABLE ONLY metric_samples ATTACH PARTITION metric_samples_p_202607 FOR VALUES FROM ('2026-07-01 00:00:00+00') TO ('2026-08-01 00:00:00+00');

ALTER TABLE ONLY metric_samples ATTACH PARTITION metric_samples_p_202608 FOR VALUES FROM ('2026-08-01 00:00:00+00') TO ('2026-09-01 00:00:00+00');

ALTER TABLE ONLY metric_samples ATTACH PARTITION metric_samples_p_202609 FOR VALUES FROM ('2026-09-01 00:00:00+00') TO ('2026-10-01 00:00:00+00');

ALTER TABLE ONLY metric_samples ATTACH PARTITION metric_samples_p_hist FOR VALUES FROM (MINVALUE) TO ('2026-04-01 00:00:00+00');

ALTER TABLE ONLY spans ATTACH PARTITION spans_p_202604 FOR VALUES FROM ('2026-04-01 00:00:00+00') TO ('2026-05-01 00:00:00+00');

ALTER TABLE ONLY spans ATTACH PARTITION spans_p_202605 FOR VALUES FROM ('2026-05-01 00:00:00+00') TO ('2026-06-01 00:00:00+00');

ALTER TABLE ONLY spans ATTACH PARTITION spans_p_202606 FOR VALUES FROM ('2026-06-01 00:00:00+00') TO ('2026-07-01 00:00:00+00');

ALTER TABLE ONLY spans ATTACH PARTITION spans_p_202607 FOR VALUES FROM ('2026-07-01 00:00:00+00') TO ('2026-08-01 00:00:00+00');

ALTER TABLE ONLY spans ATTACH PARTITION spans_p_202608 FOR VALUES FROM ('2026-08-01 00:00:00+00') TO ('2026-09-01 00:00:00+00');

ALTER TABLE ONLY spans ATTACH PARTITION spans_p_202609 FOR VALUES FROM ('2026-09-01 00:00:00+00') TO ('2026-10-01 00:00:00+00');

ALTER TABLE ONLY spans ATTACH PARTITION spans_p_hist FOR VALUES FROM (MINVALUE) TO ('2026-04-01 00:00:00+00');

ALTER TABLE ONLY agent_messages
    ADD CONSTRAINT agent_messages_pkey PRIMARY KEY (id);

ALTER TABLE ONLY agent_sessions
    ADD CONSTRAINT agent_sessions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY alert_events
    ADD CONSTRAINT alert_events_pkey PRIMARY KEY (id);

ALTER TABLE ONLY alert_rules
    ADD CONSTRAINT alert_rules_pkey PRIMARY KEY (id);

ALTER TABLE ONLY api_tokens
    ADD CONSTRAINT api_tokens_pkey PRIMARY KEY (id);

ALTER TABLE ONLY api_tokens
    ADD CONSTRAINT api_tokens_token_hash_key UNIQUE (token_hash);

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_pkey PRIMARY KEY (id);

ALTER TABLE ONLY audit_log
    ADD CONSTRAINT audit_log_pkey PRIMARY KEY (id);

ALTER TABLE ONLY auth_sessions
    ADD CONSTRAINT auth_sessions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY auth_sessions
    ADD CONSTRAINT auth_sessions_token_hash_key UNIQUE (token_hash);

ALTER TABLE ONLY branch_protection_rules
    ADD CONSTRAINT branch_protection_rules_pkey PRIMARY KEY (id);

ALTER TABLE ONLY branch_protection_rules
    ADD CONSTRAINT branch_protection_rules_project_id_pattern_key UNIQUE (project_id, pattern);

ALTER TABLE ONLY cli_credentials
    ADD CONSTRAINT cli_credentials_pkey PRIMARY KEY (id);

ALTER TABLE ONLY cli_credentials
    ADD CONSTRAINT cli_credentials_user_id_auth_type_key UNIQUE (user_id, auth_type);

ALTER TABLE ONLY comments
    ADD CONSTRAINT comments_pkey PRIMARY KEY (id);

ALTER TABLE ONLY delegations
    ADD CONSTRAINT delegations_pkey PRIMARY KEY (id);

ALTER TABLE ONLY deploy_releases
    ADD CONSTRAINT deploy_releases_pkey PRIMARY KEY (id);

ALTER TABLE ONLY deploy_targets
    ADD CONSTRAINT deploy_targets_pkey PRIMARY KEY (id);

ALTER TABLE ONLY deploy_targets
    ADD CONSTRAINT deploy_targets_project_id_environment_branch_slug_key UNIQUE NULLS NOT DISTINCT (project_id, environment, branch_slug);

ALTER TABLE ONLY feature_flag_history
    ADD CONSTRAINT feature_flag_history_pkey PRIMARY KEY (id);

ALTER TABLE ONLY feature_flag_overrides
    ADD CONSTRAINT feature_flag_overrides_flag_id_user_id_key UNIQUE (flag_id, user_id);

ALTER TABLE ONLY feature_flag_overrides
    ADD CONSTRAINT feature_flag_overrides_pkey PRIMARY KEY (id);

ALTER TABLE ONLY feature_flag_rules
    ADD CONSTRAINT feature_flag_rules_pkey PRIMARY KEY (id);

ALTER TABLE ONLY feature_flags
    ADD CONSTRAINT feature_flags_key_project_id_environment_key UNIQUE (key, project_id, environment);

ALTER TABLE ONLY feature_flags
    ADD CONSTRAINT feature_flags_pkey PRIMARY KEY (id);

ALTER TABLE ONLY issues
    ADD CONSTRAINT issues_pkey PRIMARY KEY (id);

ALTER TABLE ONLY issues
    ADD CONSTRAINT issues_project_id_number_key UNIQUE (project_id, number);

ALTER TABLE ONLY llm_provider_configs
    ADD CONSTRAINT llm_provider_configs_pkey PRIMARY KEY (id);

ALTER TABLE ONLY log_entries
    ADD CONSTRAINT log_entries_pkey PRIMARY KEY (id, "timestamp");

ALTER TABLE ONLY log_entries_p_202604
    ADD CONSTRAINT log_entries_p_202604_pkey PRIMARY KEY (id, "timestamp");

ALTER TABLE ONLY log_entries_p_202605
    ADD CONSTRAINT log_entries_p_202605_pkey PRIMARY KEY (id, "timestamp");

ALTER TABLE ONLY log_entries_p_202606
    ADD CONSTRAINT log_entries_p_202606_pkey PRIMARY KEY (id, "timestamp");

ALTER TABLE ONLY log_entries_p_202607
    ADD CONSTRAINT log_entries_p_202607_pkey PRIMARY KEY (id, "timestamp");

ALTER TABLE ONLY log_entries_p_202608
    ADD CONSTRAINT log_entries_p_202608_pkey PRIMARY KEY (id, "timestamp");

ALTER TABLE ONLY log_entries_p_202609
    ADD CONSTRAINT log_entries_p_202609_pkey PRIMARY KEY (id, "timestamp");

ALTER TABLE ONLY log_entries_p_hist
    ADD CONSTRAINT log_entries_p_hist_pkey PRIMARY KEY (id, "timestamp");

ALTER TABLE ONLY merge_requests
    ADD CONSTRAINT merge_requests_pkey PRIMARY KEY (id);

ALTER TABLE ONLY merge_requests
    ADD CONSTRAINT merge_requests_project_id_number_key UNIQUE (project_id, number);

ALTER TABLE ONLY mesh_ca
    ADD CONSTRAINT mesh_ca_pkey PRIMARY KEY (id);

ALTER TABLE ONLY mesh_certs
    ADD CONSTRAINT mesh_certs_pkey PRIMARY KEY (id);

ALTER TABLE ONLY metric_samples
    ADD CONSTRAINT metric_samples_pkey PRIMARY KEY (series_id, "timestamp");

ALTER TABLE ONLY metric_samples_p_202604
    ADD CONSTRAINT metric_samples_p_202604_pkey PRIMARY KEY (series_id, "timestamp");

ALTER TABLE ONLY metric_samples_p_202605
    ADD CONSTRAINT metric_samples_p_202605_pkey PRIMARY KEY (series_id, "timestamp");

ALTER TABLE ONLY metric_samples_p_202606
    ADD CONSTRAINT metric_samples_p_202606_pkey PRIMARY KEY (series_id, "timestamp");

ALTER TABLE ONLY metric_samples_p_202607
    ADD CONSTRAINT metric_samples_p_202607_pkey PRIMARY KEY (series_id, "timestamp");

ALTER TABLE ONLY metric_samples_p_202608
    ADD CONSTRAINT metric_samples_p_202608_pkey PRIMARY KEY (series_id, "timestamp");

ALTER TABLE ONLY metric_samples_p_202609
    ADD CONSTRAINT metric_samples_p_202609_pkey PRIMARY KEY (series_id, "timestamp");

ALTER TABLE ONLY metric_samples_p_hist
    ADD CONSTRAINT metric_samples_p_hist_pkey PRIMARY KEY (series_id, "timestamp");

ALTER TABLE ONLY metric_series
    ADD CONSTRAINT metric_series_name_labels_project_key UNIQUE NULLS NOT DISTINCT (name, labels, project_id);

ALTER TABLE ONLY metric_series
    ADD CONSTRAINT metric_series_pkey PRIMARY KEY (id);

ALTER TABLE ONLY mr_reviews
    ADD CONSTRAINT mr_reviews_pkey PRIMARY KEY (id);

ALTER TABLE ONLY notifications
    ADD CONSTRAINT notifications_pkey PRIMARY KEY (id);

ALTER TABLE ONLY ops_repos
    ADD CONSTRAINT ops_repos_name_key UNIQUE (name);

ALTER TABLE ONLY ops_repos
    ADD CONSTRAINT ops_repos_pkey PRIMARY KEY (id);

ALTER TABLE ONLY passkey_credentials
    ADD CONSTRAINT passkey_credentials_credential_id_key UNIQUE (credential_id);

ALTER TABLE ONLY passkey_credentials
    ADD CONSTRAINT passkey_credentials_pkey PRIMARY KEY (id);

ALTER TABLE ONLY permissions
    ADD CONSTRAINT permissions_name_key UNIQUE (name);

ALTER TABLE ONLY permissions
    ADD CONSTRAINT permissions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY pipeline_steps
    ADD CONSTRAINT pipeline_steps_pkey PRIMARY KEY (id);

ALTER TABLE ONLY pipelines
    ADD CONSTRAINT pipelines_pkey PRIMARY KEY (id);

ALTER TABLE ONLY platform_commands
    ADD CONSTRAINT platform_commands_pkey PRIMARY KEY (id);

ALTER TABLE ONLY platform_settings
    ADD CONSTRAINT platform_settings_pkey PRIMARY KEY (key);

ALTER TABLE ONLY projects
    ADD CONSTRAINT projects_pkey PRIMARY KEY (id);

ALTER TABLE ONLY registry_blob_links
    ADD CONSTRAINT registry_blob_links_pkey PRIMARY KEY (repository_id, blob_digest);

ALTER TABLE ONLY registry_blobs
    ADD CONSTRAINT registry_blobs_digest_key UNIQUE (digest);

ALTER TABLE ONLY registry_blobs
    ADD CONSTRAINT registry_blobs_pkey PRIMARY KEY (id);

ALTER TABLE ONLY registry_manifests
    ADD CONSTRAINT registry_manifests_pkey PRIMARY KEY (id);

ALTER TABLE ONLY registry_manifests
    ADD CONSTRAINT registry_manifests_repository_id_digest_key UNIQUE (repository_id, digest);

ALTER TABLE ONLY registry_repositories
    ADD CONSTRAINT registry_repositories_name_key UNIQUE (name);

ALTER TABLE ONLY registry_repositories
    ADD CONSTRAINT registry_repositories_pkey PRIMARY KEY (id);

ALTER TABLE ONLY registry_tags
    ADD CONSTRAINT registry_tags_pkey PRIMARY KEY (id);

ALTER TABLE ONLY registry_tags
    ADD CONSTRAINT registry_tags_repository_id_name_key UNIQUE (repository_id, name);

ALTER TABLE ONLY release_assets
    ADD CONSTRAINT release_assets_pkey PRIMARY KEY (id);

ALTER TABLE ONLY release_history
    ADD CONSTRAINT release_history_pkey PRIMARY KEY (id);

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_pkey PRIMARY KEY (id);

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_project_id_tag_name_key UNIQUE (project_id, tag_name);

ALTER TABLE ONLY role_permissions
    ADD CONSTRAINT role_permissions_pkey PRIMARY KEY (role_id, permission_id);

ALTER TABLE ONLY roles
    ADD CONSTRAINT roles_name_key UNIQUE (name);

ALTER TABLE ONLY roles
    ADD CONSTRAINT roles_pkey PRIMARY KEY (id);

ALTER TABLE ONLY rollout_analyses
    ADD CONSTRAINT rollout_analyses_pkey PRIMARY KEY (id);

ALTER TABLE ONLY secrets
    ADD CONSTRAINT secrets_pkey PRIMARY KEY (id);

ALTER TABLE ONLY setup_tokens
    ADD CONSTRAINT setup_tokens_pkey PRIMARY KEY (id);

ALTER TABLE ONLY spans
    ADD CONSTRAINT spans_pkey PRIMARY KEY (id, started_at);

ALTER TABLE ONLY spans_p_202604
    ADD CONSTRAINT spans_p_202604_pkey PRIMARY KEY (id, started_at);

ALTER TABLE ONLY spans
    ADD CONSTRAINT spans_span_id_started_at_key UNIQUE (span_id, started_at);

ALTER TABLE ONLY spans_p_202604
    ADD CONSTRAINT spans_p_202604_span_id_started_at_key UNIQUE (span_id, started_at);

ALTER TABLE ONLY spans_p_202605
    ADD CONSTRAINT spans_p_202605_pkey PRIMARY KEY (id, started_at);

ALTER TABLE ONLY spans_p_202605
    ADD CONSTRAINT spans_p_202605_span_id_started_at_key UNIQUE (span_id, started_at);

ALTER TABLE ONLY spans_p_202606
    ADD CONSTRAINT spans_p_202606_pkey PRIMARY KEY (id, started_at);

ALTER TABLE ONLY spans_p_202606
    ADD CONSTRAINT spans_p_202606_span_id_started_at_key UNIQUE (span_id, started_at);

ALTER TABLE ONLY spans_p_202607
    ADD CONSTRAINT spans_p_202607_pkey PRIMARY KEY (id, started_at);

ALTER TABLE ONLY spans_p_202607
    ADD CONSTRAINT spans_p_202607_span_id_started_at_key UNIQUE (span_id, started_at);

ALTER TABLE ONLY spans_p_202608
    ADD CONSTRAINT spans_p_202608_pkey PRIMARY KEY (id, started_at);

ALTER TABLE ONLY spans_p_202608
    ADD CONSTRAINT spans_p_202608_span_id_started_at_key UNIQUE (span_id, started_at);

ALTER TABLE ONLY spans_p_202609
    ADD CONSTRAINT spans_p_202609_pkey PRIMARY KEY (id, started_at);

ALTER TABLE ONLY spans_p_202609
    ADD CONSTRAINT spans_p_202609_span_id_started_at_key UNIQUE (span_id, started_at);

ALTER TABLE ONLY spans_p_hist
    ADD CONSTRAINT spans_p_hist_pkey PRIMARY KEY (id, started_at);

ALTER TABLE ONLY spans_p_hist
    ADD CONSTRAINT spans_p_hist_span_id_started_at_key UNIQUE (span_id, started_at);

ALTER TABLE ONLY traces
    ADD CONSTRAINT traces_pkey PRIMARY KEY (id);

ALTER TABLE ONLY traces
    ADD CONSTRAINT traces_trace_id_key UNIQUE (trace_id);

ALTER TABLE ONLY delegations
    ADD CONSTRAINT uq_delegations_unique_grant UNIQUE NULLS NOT DISTINCT (delegator_id, delegate_id, permission_id, project_id);

ALTER TABLE ONLY user_gpg_keys
    ADD CONSTRAINT user_gpg_keys_fingerprint_key UNIQUE (fingerprint);

ALTER TABLE ONLY user_gpg_keys
    ADD CONSTRAINT user_gpg_keys_pkey PRIMARY KEY (id);

ALTER TABLE ONLY user_provider_keys
    ADD CONSTRAINT user_provider_keys_pkey PRIMARY KEY (id);

ALTER TABLE ONLY user_provider_keys
    ADD CONSTRAINT user_provider_keys_user_id_provider_key UNIQUE (user_id, provider);

ALTER TABLE ONLY user_roles
    ADD CONSTRAINT user_roles_pkey PRIMARY KEY (id);

ALTER TABLE ONLY user_roles
    ADD CONSTRAINT user_roles_user_id_role_id_project_id_key UNIQUE NULLS NOT DISTINCT (user_id, role_id, project_id);

ALTER TABLE ONLY user_ssh_keys
    ADD CONSTRAINT user_ssh_keys_fingerprint_key UNIQUE (fingerprint);

ALTER TABLE ONLY user_ssh_keys
    ADD CONSTRAINT user_ssh_keys_pkey PRIMARY KEY (id);

ALTER TABLE ONLY users
    ADD CONSTRAINT users_email_key UNIQUE (email);

ALTER TABLE ONLY users
    ADD CONSTRAINT users_name_key UNIQUE (name);

ALTER TABLE ONLY users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);

ALTER TABLE ONLY webhooks
    ADD CONSTRAINT webhooks_pkey PRIMARY KEY (id);

ALTER TABLE ONLY workspace_members
    ADD CONSTRAINT workspace_members_pkey PRIMARY KEY (id);

ALTER TABLE ONLY workspace_members
    ADD CONSTRAINT workspace_members_workspace_id_user_id_key UNIQUE (workspace_id, user_id);

ALTER TABLE ONLY workspaces
    ADD CONSTRAINT workspaces_name_key UNIQUE (name);

ALTER TABLE ONLY workspaces
    ADD CONSTRAINT workspaces_pkey PRIMARY KEY (id);

CREATE INDEX idx_agent_messages_session ON agent_messages USING btree (session_id, created_at);

CREATE INDEX idx_agent_sessions_project ON agent_sessions USING btree (project_id);

CREATE INDEX idx_agent_sessions_status ON agent_sessions USING btree (status);

CREATE INDEX idx_agent_sessions_user ON agent_sessions USING btree (user_id);

CREATE INDEX idx_alert_events_status ON alert_events USING btree (status, created_at DESC);

CREATE INDEX idx_alert_rules_project ON alert_rules USING btree (project_id);

CREATE INDEX idx_api_tokens_scope_project ON api_tokens USING btree (project_id) WHERE (project_id IS NOT NULL);

CREATE INDEX idx_api_tokens_scope_workspace ON api_tokens USING btree (scope_workspace_id) WHERE (scope_workspace_id IS NOT NULL);

CREATE INDEX idx_api_tokens_user ON api_tokens USING btree (user_id);

CREATE INDEX idx_artifacts_expires_at ON artifacts USING btree (expires_at) WHERE (expires_at IS NOT NULL);

CREATE INDEX idx_artifacts_parent ON artifacts USING btree (parent_id);

CREATE INDEX idx_artifacts_type ON artifacts USING btree (pipeline_id, artifact_type);

CREATE INDEX idx_audit_actor ON audit_log USING btree (actor_id, created_at DESC);

CREATE INDEX idx_audit_resource ON audit_log USING btree (resource, resource_id, created_at DESC);

CREATE INDEX idx_auth_sessions_user ON auth_sessions USING btree (user_id);

CREATE INDEX idx_cli_credentials_user ON cli_credentials USING btree (user_id);

CREATE INDEX idx_comments_issue ON comments USING btree (issue_id, created_at) WHERE (issue_id IS NOT NULL);

CREATE INDEX idx_comments_mr ON comments USING btree (mr_id, created_at) WHERE (mr_id IS NOT NULL);

CREATE INDEX idx_delegations_delegate ON delegations USING btree (delegate_id);

CREATE INDEX idx_deploy_releases_project_started ON deploy_releases USING btree (project_id, started_at);

CREATE INDEX idx_deploy_releases_reconcile ON deploy_releases USING btree (phase) WHERE (phase = ANY (ARRAY['pending'::text, 'progressing'::text, 'holding'::text, 'promoting'::text, 'rolling_back'::text]));

CREATE INDEX idx_deploy_releases_target ON deploy_releases USING btree (target_id, created_at DESC);

CREATE INDEX idx_feature_flag_history_flag ON feature_flag_history USING btree (flag_id, created_at DESC);

CREATE INDEX idx_llm_provider_configs_user ON llm_provider_configs USING btree (user_id);

CREATE INDEX idx_log_attrs ON ONLY log_entries USING gin (attributes jsonb_path_ops);

CREATE INDEX idx_log_level ON ONLY log_entries USING btree (level, "timestamp" DESC);

CREATE INDEX idx_log_project ON ONLY log_entries USING btree (project_id, "timestamp" DESC);

CREATE INDEX idx_log_session ON ONLY log_entries USING btree (session_id, "timestamp" DESC);

CREATE INDEX idx_log_source ON ONLY log_entries USING btree (source, "timestamp" DESC);

CREATE INDEX idx_log_trace ON ONLY log_entries USING btree (trace_id);

CREATE INDEX idx_log_ts ON ONLY log_entries USING btree ("timestamp" DESC);

CREATE INDEX idx_mesh_certs_ca_id ON mesh_certs USING btree (ca_id);

CREATE INDEX idx_mesh_certs_spiffe_id ON mesh_certs USING btree (spiffe_id);

CREATE INDEX idx_mr_reviews_mr ON mr_reviews USING btree (mr_id, created_at);

CREATE INDEX idx_notifications_user_status ON notifications USING btree (user_id, status, created_at DESC);

CREATE UNIQUE INDEX idx_ops_repos_project ON ops_repos USING btree (project_id) WHERE (project_id IS NOT NULL);

CREATE INDEX idx_passkey_credentials_user ON passkey_credentials USING btree (user_id);

CREATE INDEX idx_pipeline_steps_finished ON pipeline_steps USING btree (finished_at) WHERE (finished_at IS NOT NULL);

CREATE INDEX idx_pipelines_project ON pipelines USING btree (project_id, created_at DESC);

CREATE INDEX idx_pipelines_project_status ON pipelines USING btree (project_id, status, created_at DESC);

CREATE INDEX idx_pipelines_status ON pipelines USING btree (status);

CREATE UNIQUE INDEX idx_platform_commands_scoped ON platform_commands USING btree (COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000'::uuid), COALESCE(project_id, '00000000-0000-0000-0000-000000000000'::uuid), name);

CREATE UNIQUE INDEX idx_projects_namespace_slug ON projects USING btree (namespace_slug) WHERE (is_active = true);

CREATE UNIQUE INDEX idx_projects_owner_name_active ON projects USING btree (owner_id, name) WHERE (is_active = true);

CREATE INDEX idx_projects_workspace ON projects USING btree (workspace_id) WHERE (workspace_id IS NOT NULL);

CREATE INDEX idx_registry_repos_project ON registry_repositories USING btree (project_id);

CREATE INDEX idx_release_history_release ON release_history USING btree (release_id, created_at DESC);

CREATE UNIQUE INDEX idx_secrets_global_name ON secrets USING btree (name) WHERE ((project_id IS NULL) AND (workspace_id IS NULL) AND (environment IS NULL));

CREATE UNIQUE INDEX idx_secrets_scoped ON secrets USING btree (COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000'::uuid), COALESCE(project_id, '00000000-0000-0000-0000-000000000000'::uuid), COALESCE(environment, '__none__'::text), name);

CREATE INDEX idx_sessions_parent ON agent_sessions USING btree (parent_session_id) WHERE (parent_session_id IS NOT NULL);

CREATE INDEX idx_spans_project_kind_started ON ONLY spans USING btree (project_id, kind, started_at);

CREATE INDEX idx_spans_session_started ON ONLY spans USING btree (session_id, started_at) WHERE (session_id IS NOT NULL);

CREATE INDEX idx_spans_status_kind_started ON ONLY spans USING btree (status, kind, started_at);

CREATE INDEX idx_spans_trace ON ONLY spans USING btree (trace_id);

CREATE INDEX idx_traces_project_started ON traces USING btree (project_id, started_at);

CREATE INDEX idx_traces_started ON traces USING btree (started_at);

CREATE INDEX idx_user_gpg_keys_emails ON user_gpg_keys USING gin (emails);

CREATE INDEX idx_user_gpg_keys_key_id ON user_gpg_keys USING btree (key_id);

CREATE INDEX idx_user_gpg_keys_user ON user_gpg_keys USING btree (user_id);

CREATE INDEX idx_user_roles_user ON user_roles USING btree (user_id);

CREATE INDEX idx_user_ssh_keys_user ON user_ssh_keys USING btree (user_id);

CREATE INDEX idx_users_user_type ON users USING btree (user_type);

CREATE INDEX idx_webhooks_project ON webhooks USING btree (project_id) WHERE (active = true);

CREATE INDEX log_entries_p_202604_attributes_idx ON log_entries_p_202604 USING gin (attributes jsonb_path_ops);

CREATE INDEX log_entries_p_202604_level_timestamp_idx ON log_entries_p_202604 USING btree (level, "timestamp" DESC);

CREATE INDEX log_entries_p_202604_project_id_timestamp_idx ON log_entries_p_202604 USING btree (project_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202604_session_id_timestamp_idx ON log_entries_p_202604 USING btree (session_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202604_source_timestamp_idx ON log_entries_p_202604 USING btree (source, "timestamp" DESC);

CREATE INDEX log_entries_p_202604_timestamp_idx ON log_entries_p_202604 USING btree ("timestamp" DESC);

CREATE INDEX log_entries_p_202604_trace_id_idx ON log_entries_p_202604 USING btree (trace_id);

CREATE INDEX log_entries_p_202605_attributes_idx ON log_entries_p_202605 USING gin (attributes jsonb_path_ops);

CREATE INDEX log_entries_p_202605_level_timestamp_idx ON log_entries_p_202605 USING btree (level, "timestamp" DESC);

CREATE INDEX log_entries_p_202605_project_id_timestamp_idx ON log_entries_p_202605 USING btree (project_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202605_session_id_timestamp_idx ON log_entries_p_202605 USING btree (session_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202605_source_timestamp_idx ON log_entries_p_202605 USING btree (source, "timestamp" DESC);

CREATE INDEX log_entries_p_202605_timestamp_idx ON log_entries_p_202605 USING btree ("timestamp" DESC);

CREATE INDEX log_entries_p_202605_trace_id_idx ON log_entries_p_202605 USING btree (trace_id);

CREATE INDEX log_entries_p_202606_attributes_idx ON log_entries_p_202606 USING gin (attributes jsonb_path_ops);

CREATE INDEX log_entries_p_202606_level_timestamp_idx ON log_entries_p_202606 USING btree (level, "timestamp" DESC);

CREATE INDEX log_entries_p_202606_project_id_timestamp_idx ON log_entries_p_202606 USING btree (project_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202606_session_id_timestamp_idx ON log_entries_p_202606 USING btree (session_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202606_source_timestamp_idx ON log_entries_p_202606 USING btree (source, "timestamp" DESC);

CREATE INDEX log_entries_p_202606_timestamp_idx ON log_entries_p_202606 USING btree ("timestamp" DESC);

CREATE INDEX log_entries_p_202606_trace_id_idx ON log_entries_p_202606 USING btree (trace_id);

CREATE INDEX log_entries_p_202607_attributes_idx ON log_entries_p_202607 USING gin (attributes jsonb_path_ops);

CREATE INDEX log_entries_p_202607_level_timestamp_idx ON log_entries_p_202607 USING btree (level, "timestamp" DESC);

CREATE INDEX log_entries_p_202607_project_id_timestamp_idx ON log_entries_p_202607 USING btree (project_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202607_session_id_timestamp_idx ON log_entries_p_202607 USING btree (session_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202607_source_timestamp_idx ON log_entries_p_202607 USING btree (source, "timestamp" DESC);

CREATE INDEX log_entries_p_202607_timestamp_idx ON log_entries_p_202607 USING btree ("timestamp" DESC);

CREATE INDEX log_entries_p_202607_trace_id_idx ON log_entries_p_202607 USING btree (trace_id);

CREATE INDEX log_entries_p_202608_attributes_idx ON log_entries_p_202608 USING gin (attributes jsonb_path_ops);

CREATE INDEX log_entries_p_202608_level_timestamp_idx ON log_entries_p_202608 USING btree (level, "timestamp" DESC);

CREATE INDEX log_entries_p_202608_project_id_timestamp_idx ON log_entries_p_202608 USING btree (project_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202608_session_id_timestamp_idx ON log_entries_p_202608 USING btree (session_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202608_source_timestamp_idx ON log_entries_p_202608 USING btree (source, "timestamp" DESC);

CREATE INDEX log_entries_p_202608_timestamp_idx ON log_entries_p_202608 USING btree ("timestamp" DESC);

CREATE INDEX log_entries_p_202608_trace_id_idx ON log_entries_p_202608 USING btree (trace_id);

CREATE INDEX log_entries_p_202609_attributes_idx ON log_entries_p_202609 USING gin (attributes jsonb_path_ops);

CREATE INDEX log_entries_p_202609_level_timestamp_idx ON log_entries_p_202609 USING btree (level, "timestamp" DESC);

CREATE INDEX log_entries_p_202609_project_id_timestamp_idx ON log_entries_p_202609 USING btree (project_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202609_session_id_timestamp_idx ON log_entries_p_202609 USING btree (session_id, "timestamp" DESC);

CREATE INDEX log_entries_p_202609_source_timestamp_idx ON log_entries_p_202609 USING btree (source, "timestamp" DESC);

CREATE INDEX log_entries_p_202609_timestamp_idx ON log_entries_p_202609 USING btree ("timestamp" DESC);

CREATE INDEX log_entries_p_202609_trace_id_idx ON log_entries_p_202609 USING btree (trace_id);

CREATE INDEX log_entries_p_hist_attributes_idx ON log_entries_p_hist USING gin (attributes jsonb_path_ops);

CREATE INDEX log_entries_p_hist_level_timestamp_idx ON log_entries_p_hist USING btree (level, "timestamp" DESC);

CREATE INDEX log_entries_p_hist_project_id_timestamp_idx ON log_entries_p_hist USING btree (project_id, "timestamp" DESC);

CREATE INDEX log_entries_p_hist_session_id_timestamp_idx ON log_entries_p_hist USING btree (session_id, "timestamp" DESC);

CREATE INDEX log_entries_p_hist_source_timestamp_idx ON log_entries_p_hist USING btree (source, "timestamp" DESC);

CREATE INDEX log_entries_p_hist_timestamp_idx ON log_entries_p_hist USING btree ("timestamp" DESC);

CREATE INDEX log_entries_p_hist_trace_id_idx ON log_entries_p_hist USING btree (trace_id);

CREATE INDEX spans_p_202604_project_id_kind_started_at_idx ON spans_p_202604 USING btree (project_id, kind, started_at);

CREATE INDEX spans_p_202604_session_id_started_at_idx ON spans_p_202604 USING btree (session_id, started_at) WHERE (session_id IS NOT NULL);

CREATE INDEX spans_p_202604_status_kind_started_at_idx ON spans_p_202604 USING btree (status, kind, started_at);

CREATE INDEX spans_p_202604_trace_id_idx ON spans_p_202604 USING btree (trace_id);

CREATE INDEX spans_p_202605_project_id_kind_started_at_idx ON spans_p_202605 USING btree (project_id, kind, started_at);

CREATE INDEX spans_p_202605_session_id_started_at_idx ON spans_p_202605 USING btree (session_id, started_at) WHERE (session_id IS NOT NULL);

CREATE INDEX spans_p_202605_status_kind_started_at_idx ON spans_p_202605 USING btree (status, kind, started_at);

CREATE INDEX spans_p_202605_trace_id_idx ON spans_p_202605 USING btree (trace_id);

CREATE INDEX spans_p_202606_project_id_kind_started_at_idx ON spans_p_202606 USING btree (project_id, kind, started_at);

CREATE INDEX spans_p_202606_session_id_started_at_idx ON spans_p_202606 USING btree (session_id, started_at) WHERE (session_id IS NOT NULL);

CREATE INDEX spans_p_202606_status_kind_started_at_idx ON spans_p_202606 USING btree (status, kind, started_at);

CREATE INDEX spans_p_202606_trace_id_idx ON spans_p_202606 USING btree (trace_id);

CREATE INDEX spans_p_202607_project_id_kind_started_at_idx ON spans_p_202607 USING btree (project_id, kind, started_at);

CREATE INDEX spans_p_202607_session_id_started_at_idx ON spans_p_202607 USING btree (session_id, started_at) WHERE (session_id IS NOT NULL);

CREATE INDEX spans_p_202607_status_kind_started_at_idx ON spans_p_202607 USING btree (status, kind, started_at);

CREATE INDEX spans_p_202607_trace_id_idx ON spans_p_202607 USING btree (trace_id);

CREATE INDEX spans_p_202608_project_id_kind_started_at_idx ON spans_p_202608 USING btree (project_id, kind, started_at);

CREATE INDEX spans_p_202608_session_id_started_at_idx ON spans_p_202608 USING btree (session_id, started_at) WHERE (session_id IS NOT NULL);

CREATE INDEX spans_p_202608_status_kind_started_at_idx ON spans_p_202608 USING btree (status, kind, started_at);

CREATE INDEX spans_p_202608_trace_id_idx ON spans_p_202608 USING btree (trace_id);

CREATE INDEX spans_p_202609_project_id_kind_started_at_idx ON spans_p_202609 USING btree (project_id, kind, started_at);

CREATE INDEX spans_p_202609_session_id_started_at_idx ON spans_p_202609 USING btree (session_id, started_at) WHERE (session_id IS NOT NULL);

CREATE INDEX spans_p_202609_status_kind_started_at_idx ON spans_p_202609 USING btree (status, kind, started_at);

CREATE INDEX spans_p_202609_trace_id_idx ON spans_p_202609 USING btree (trace_id);

CREATE INDEX spans_p_hist_project_id_kind_started_at_idx ON spans_p_hist USING btree (project_id, kind, started_at);

CREATE INDEX spans_p_hist_session_id_started_at_idx ON spans_p_hist USING btree (session_id, started_at) WHERE (session_id IS NOT NULL);

CREATE INDEX spans_p_hist_status_kind_started_at_idx ON spans_p_hist USING btree (status, kind, started_at);

CREATE INDEX spans_p_hist_trace_id_idx ON spans_p_hist USING btree (trace_id);

ALTER INDEX idx_log_attrs ATTACH PARTITION log_entries_p_202604_attributes_idx;

ALTER INDEX idx_log_level ATTACH PARTITION log_entries_p_202604_level_timestamp_idx;

ALTER INDEX log_entries_pkey ATTACH PARTITION log_entries_p_202604_pkey;

ALTER INDEX idx_log_project ATTACH PARTITION log_entries_p_202604_project_id_timestamp_idx;

ALTER INDEX idx_log_session ATTACH PARTITION log_entries_p_202604_session_id_timestamp_idx;

ALTER INDEX idx_log_source ATTACH PARTITION log_entries_p_202604_source_timestamp_idx;

ALTER INDEX idx_log_ts ATTACH PARTITION log_entries_p_202604_timestamp_idx;

ALTER INDEX idx_log_trace ATTACH PARTITION log_entries_p_202604_trace_id_idx;

ALTER INDEX idx_log_attrs ATTACH PARTITION log_entries_p_202605_attributes_idx;

ALTER INDEX idx_log_level ATTACH PARTITION log_entries_p_202605_level_timestamp_idx;

ALTER INDEX log_entries_pkey ATTACH PARTITION log_entries_p_202605_pkey;

ALTER INDEX idx_log_project ATTACH PARTITION log_entries_p_202605_project_id_timestamp_idx;

ALTER INDEX idx_log_session ATTACH PARTITION log_entries_p_202605_session_id_timestamp_idx;

ALTER INDEX idx_log_source ATTACH PARTITION log_entries_p_202605_source_timestamp_idx;

ALTER INDEX idx_log_ts ATTACH PARTITION log_entries_p_202605_timestamp_idx;

ALTER INDEX idx_log_trace ATTACH PARTITION log_entries_p_202605_trace_id_idx;

ALTER INDEX idx_log_attrs ATTACH PARTITION log_entries_p_202606_attributes_idx;

ALTER INDEX idx_log_level ATTACH PARTITION log_entries_p_202606_level_timestamp_idx;

ALTER INDEX log_entries_pkey ATTACH PARTITION log_entries_p_202606_pkey;

ALTER INDEX idx_log_project ATTACH PARTITION log_entries_p_202606_project_id_timestamp_idx;

ALTER INDEX idx_log_session ATTACH PARTITION log_entries_p_202606_session_id_timestamp_idx;

ALTER INDEX idx_log_source ATTACH PARTITION log_entries_p_202606_source_timestamp_idx;

ALTER INDEX idx_log_ts ATTACH PARTITION log_entries_p_202606_timestamp_idx;

ALTER INDEX idx_log_trace ATTACH PARTITION log_entries_p_202606_trace_id_idx;

ALTER INDEX idx_log_attrs ATTACH PARTITION log_entries_p_202607_attributes_idx;

ALTER INDEX idx_log_level ATTACH PARTITION log_entries_p_202607_level_timestamp_idx;

ALTER INDEX log_entries_pkey ATTACH PARTITION log_entries_p_202607_pkey;

ALTER INDEX idx_log_project ATTACH PARTITION log_entries_p_202607_project_id_timestamp_idx;

ALTER INDEX idx_log_session ATTACH PARTITION log_entries_p_202607_session_id_timestamp_idx;

ALTER INDEX idx_log_source ATTACH PARTITION log_entries_p_202607_source_timestamp_idx;

ALTER INDEX idx_log_ts ATTACH PARTITION log_entries_p_202607_timestamp_idx;

ALTER INDEX idx_log_trace ATTACH PARTITION log_entries_p_202607_trace_id_idx;

ALTER INDEX idx_log_attrs ATTACH PARTITION log_entries_p_202608_attributes_idx;

ALTER INDEX idx_log_level ATTACH PARTITION log_entries_p_202608_level_timestamp_idx;

ALTER INDEX log_entries_pkey ATTACH PARTITION log_entries_p_202608_pkey;

ALTER INDEX idx_log_project ATTACH PARTITION log_entries_p_202608_project_id_timestamp_idx;

ALTER INDEX idx_log_session ATTACH PARTITION log_entries_p_202608_session_id_timestamp_idx;

ALTER INDEX idx_log_source ATTACH PARTITION log_entries_p_202608_source_timestamp_idx;

ALTER INDEX idx_log_ts ATTACH PARTITION log_entries_p_202608_timestamp_idx;

ALTER INDEX idx_log_trace ATTACH PARTITION log_entries_p_202608_trace_id_idx;

ALTER INDEX idx_log_attrs ATTACH PARTITION log_entries_p_202609_attributes_idx;

ALTER INDEX idx_log_level ATTACH PARTITION log_entries_p_202609_level_timestamp_idx;

ALTER INDEX log_entries_pkey ATTACH PARTITION log_entries_p_202609_pkey;

ALTER INDEX idx_log_project ATTACH PARTITION log_entries_p_202609_project_id_timestamp_idx;

ALTER INDEX idx_log_session ATTACH PARTITION log_entries_p_202609_session_id_timestamp_idx;

ALTER INDEX idx_log_source ATTACH PARTITION log_entries_p_202609_source_timestamp_idx;

ALTER INDEX idx_log_ts ATTACH PARTITION log_entries_p_202609_timestamp_idx;

ALTER INDEX idx_log_trace ATTACH PARTITION log_entries_p_202609_trace_id_idx;

ALTER INDEX idx_log_attrs ATTACH PARTITION log_entries_p_hist_attributes_idx;

ALTER INDEX idx_log_level ATTACH PARTITION log_entries_p_hist_level_timestamp_idx;

ALTER INDEX log_entries_pkey ATTACH PARTITION log_entries_p_hist_pkey;

ALTER INDEX idx_log_project ATTACH PARTITION log_entries_p_hist_project_id_timestamp_idx;

ALTER INDEX idx_log_session ATTACH PARTITION log_entries_p_hist_session_id_timestamp_idx;

ALTER INDEX idx_log_source ATTACH PARTITION log_entries_p_hist_source_timestamp_idx;

ALTER INDEX idx_log_ts ATTACH PARTITION log_entries_p_hist_timestamp_idx;

ALTER INDEX idx_log_trace ATTACH PARTITION log_entries_p_hist_trace_id_idx;

ALTER INDEX metric_samples_pkey ATTACH PARTITION metric_samples_p_202604_pkey;

ALTER INDEX metric_samples_pkey ATTACH PARTITION metric_samples_p_202605_pkey;

ALTER INDEX metric_samples_pkey ATTACH PARTITION metric_samples_p_202606_pkey;

ALTER INDEX metric_samples_pkey ATTACH PARTITION metric_samples_p_202607_pkey;

ALTER INDEX metric_samples_pkey ATTACH PARTITION metric_samples_p_202608_pkey;

ALTER INDEX metric_samples_pkey ATTACH PARTITION metric_samples_p_202609_pkey;

ALTER INDEX metric_samples_pkey ATTACH PARTITION metric_samples_p_hist_pkey;

ALTER INDEX spans_pkey ATTACH PARTITION spans_p_202604_pkey;

ALTER INDEX idx_spans_project_kind_started ATTACH PARTITION spans_p_202604_project_id_kind_started_at_idx;

ALTER INDEX idx_spans_session_started ATTACH PARTITION spans_p_202604_session_id_started_at_idx;

ALTER INDEX spans_span_id_started_at_key ATTACH PARTITION spans_p_202604_span_id_started_at_key;

ALTER INDEX idx_spans_status_kind_started ATTACH PARTITION spans_p_202604_status_kind_started_at_idx;

ALTER INDEX idx_spans_trace ATTACH PARTITION spans_p_202604_trace_id_idx;

ALTER INDEX spans_pkey ATTACH PARTITION spans_p_202605_pkey;

ALTER INDEX idx_spans_project_kind_started ATTACH PARTITION spans_p_202605_project_id_kind_started_at_idx;

ALTER INDEX idx_spans_session_started ATTACH PARTITION spans_p_202605_session_id_started_at_idx;

ALTER INDEX spans_span_id_started_at_key ATTACH PARTITION spans_p_202605_span_id_started_at_key;

ALTER INDEX idx_spans_status_kind_started ATTACH PARTITION spans_p_202605_status_kind_started_at_idx;

ALTER INDEX idx_spans_trace ATTACH PARTITION spans_p_202605_trace_id_idx;

ALTER INDEX spans_pkey ATTACH PARTITION spans_p_202606_pkey;

ALTER INDEX idx_spans_project_kind_started ATTACH PARTITION spans_p_202606_project_id_kind_started_at_idx;

ALTER INDEX idx_spans_session_started ATTACH PARTITION spans_p_202606_session_id_started_at_idx;

ALTER INDEX spans_span_id_started_at_key ATTACH PARTITION spans_p_202606_span_id_started_at_key;

ALTER INDEX idx_spans_status_kind_started ATTACH PARTITION spans_p_202606_status_kind_started_at_idx;

ALTER INDEX idx_spans_trace ATTACH PARTITION spans_p_202606_trace_id_idx;

ALTER INDEX spans_pkey ATTACH PARTITION spans_p_202607_pkey;

ALTER INDEX idx_spans_project_kind_started ATTACH PARTITION spans_p_202607_project_id_kind_started_at_idx;

ALTER INDEX idx_spans_session_started ATTACH PARTITION spans_p_202607_session_id_started_at_idx;

ALTER INDEX spans_span_id_started_at_key ATTACH PARTITION spans_p_202607_span_id_started_at_key;

ALTER INDEX idx_spans_status_kind_started ATTACH PARTITION spans_p_202607_status_kind_started_at_idx;

ALTER INDEX idx_spans_trace ATTACH PARTITION spans_p_202607_trace_id_idx;

ALTER INDEX spans_pkey ATTACH PARTITION spans_p_202608_pkey;

ALTER INDEX idx_spans_project_kind_started ATTACH PARTITION spans_p_202608_project_id_kind_started_at_idx;

ALTER INDEX idx_spans_session_started ATTACH PARTITION spans_p_202608_session_id_started_at_idx;

ALTER INDEX spans_span_id_started_at_key ATTACH PARTITION spans_p_202608_span_id_started_at_key;

ALTER INDEX idx_spans_status_kind_started ATTACH PARTITION spans_p_202608_status_kind_started_at_idx;

ALTER INDEX idx_spans_trace ATTACH PARTITION spans_p_202608_trace_id_idx;

ALTER INDEX spans_pkey ATTACH PARTITION spans_p_202609_pkey;

ALTER INDEX idx_spans_project_kind_started ATTACH PARTITION spans_p_202609_project_id_kind_started_at_idx;

ALTER INDEX idx_spans_session_started ATTACH PARTITION spans_p_202609_session_id_started_at_idx;

ALTER INDEX spans_span_id_started_at_key ATTACH PARTITION spans_p_202609_span_id_started_at_key;

ALTER INDEX idx_spans_status_kind_started ATTACH PARTITION spans_p_202609_status_kind_started_at_idx;

ALTER INDEX idx_spans_trace ATTACH PARTITION spans_p_202609_trace_id_idx;

ALTER INDEX spans_pkey ATTACH PARTITION spans_p_hist_pkey;

ALTER INDEX idx_spans_project_kind_started ATTACH PARTITION spans_p_hist_project_id_kind_started_at_idx;

ALTER INDEX idx_spans_session_started ATTACH PARTITION spans_p_hist_session_id_started_at_idx;

ALTER INDEX spans_span_id_started_at_key ATTACH PARTITION spans_p_hist_span_id_started_at_key;

ALTER INDEX idx_spans_status_kind_started ATTACH PARTITION spans_p_hist_status_kind_started_at_idx;

ALTER INDEX idx_spans_trace ATTACH PARTITION spans_p_hist_trace_id_idx;

CREATE TRIGGER trg_alert_rules_updated_at BEFORE UPDATE ON alert_rules FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_api_tokens_updated_at BEFORE UPDATE ON api_tokens FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_comments_updated_at BEFORE UPDATE ON comments FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_deploy_releases_updated_at BEFORE UPDATE ON deploy_releases FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_deploy_targets_updated_at BEFORE UPDATE ON deploy_targets FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_feature_flags_updated_at BEFORE UPDATE ON feature_flags FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_issues_updated_at BEFORE UPDATE ON issues FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_merge_requests_updated_at BEFORE UPDATE ON merge_requests FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_metric_series_updated_at BEFORE UPDATE ON metric_series FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_projects_updated_at BEFORE UPDATE ON projects FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_registry_repos_updated BEFORE UPDATE ON registry_repositories FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_registry_tags_updated BEFORE UPDATE ON registry_tags FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_secrets_updated_at BEFORE UPDATE ON secrets FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_users_updated_at BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_webhooks_updated_at BEFORE UPDATE ON webhooks FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE ONLY agent_messages
    ADD CONSTRAINT agent_messages_session_id_fkey FOREIGN KEY (session_id) REFERENCES agent_sessions(id) ON DELETE CASCADE;

ALTER TABLE ONLY agent_sessions
    ADD CONSTRAINT agent_sessions_agent_user_id_fkey FOREIGN KEY (agent_user_id) REFERENCES users(id);

ALTER TABLE ONLY agent_sessions
    ADD CONSTRAINT agent_sessions_parent_session_id_fkey FOREIGN KEY (parent_session_id) REFERENCES agent_sessions(id);

ALTER TABLE ONLY agent_sessions
    ADD CONSTRAINT agent_sessions_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY agent_sessions
    ADD CONSTRAINT agent_sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id);

ALTER TABLE ONLY alert_events
    ADD CONSTRAINT alert_events_rule_id_fkey FOREIGN KEY (rule_id) REFERENCES alert_rules(id) ON DELETE CASCADE;

ALTER TABLE ONLY alert_rules
    ADD CONSTRAINT alert_rules_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id);

ALTER TABLE ONLY api_tokens
    ADD CONSTRAINT api_tokens_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY api_tokens
    ADD CONSTRAINT api_tokens_scope_workspace_id_fkey FOREIGN KEY (scope_workspace_id) REFERENCES workspaces(id);

ALTER TABLE ONLY api_tokens
    ADD CONSTRAINT api_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES artifacts(id) ON DELETE CASCADE;

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_pipeline_id_fkey FOREIGN KEY (pipeline_id) REFERENCES pipelines(id) ON DELETE CASCADE;

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_step_id_fkey FOREIGN KEY (step_id) REFERENCES pipeline_steps(id) ON DELETE CASCADE;

ALTER TABLE ONLY audit_log
    ADD CONSTRAINT audit_log_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;

ALTER TABLE ONLY auth_sessions
    ADD CONSTRAINT auth_sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY branch_protection_rules
    ADD CONSTRAINT branch_protection_rules_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY cli_credentials
    ADD CONSTRAINT cli_credentials_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY comments
    ADD CONSTRAINT comments_author_id_fkey FOREIGN KEY (author_id) REFERENCES users(id);

ALTER TABLE ONLY comments
    ADD CONSTRAINT comments_issue_id_fkey FOREIGN KEY (issue_id) REFERENCES issues(id) ON DELETE CASCADE;

ALTER TABLE ONLY comments
    ADD CONSTRAINT comments_mr_id_fkey FOREIGN KEY (mr_id) REFERENCES merge_requests(id) ON DELETE CASCADE;

ALTER TABLE ONLY comments
    ADD CONSTRAINT comments_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY delegations
    ADD CONSTRAINT delegations_delegate_id_fkey FOREIGN KEY (delegate_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY delegations
    ADD CONSTRAINT delegations_delegator_id_fkey FOREIGN KEY (delegator_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY delegations
    ADD CONSTRAINT delegations_permission_id_fkey FOREIGN KEY (permission_id) REFERENCES permissions(id) ON DELETE CASCADE;

ALTER TABLE ONLY delegations
    ADD CONSTRAINT delegations_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY deploy_releases
    ADD CONSTRAINT deploy_releases_deployed_by_fkey FOREIGN KEY (deployed_by) REFERENCES users(id);

ALTER TABLE ONLY deploy_releases
    ADD CONSTRAINT deploy_releases_pipeline_id_fkey FOREIGN KEY (pipeline_id) REFERENCES pipelines(id);

ALTER TABLE ONLY deploy_releases
    ADD CONSTRAINT deploy_releases_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY deploy_releases
    ADD CONSTRAINT deploy_releases_target_id_fkey FOREIGN KEY (target_id) REFERENCES deploy_targets(id) ON DELETE CASCADE;

ALTER TABLE ONLY deploy_targets
    ADD CONSTRAINT deploy_targets_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id);

ALTER TABLE ONLY deploy_targets
    ADD CONSTRAINT deploy_targets_ops_repo_id_fkey FOREIGN KEY (ops_repo_id) REFERENCES ops_repos(id);

ALTER TABLE ONLY deploy_targets
    ADD CONSTRAINT deploy_targets_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY feature_flag_history
    ADD CONSTRAINT feature_flag_history_actor_id_fkey FOREIGN KEY (actor_id) REFERENCES users(id);

ALTER TABLE ONLY feature_flag_history
    ADD CONSTRAINT feature_flag_history_flag_id_fkey FOREIGN KEY (flag_id) REFERENCES feature_flags(id) ON DELETE CASCADE;

ALTER TABLE ONLY feature_flag_overrides
    ADD CONSTRAINT feature_flag_overrides_flag_id_fkey FOREIGN KEY (flag_id) REFERENCES feature_flags(id) ON DELETE CASCADE;

ALTER TABLE ONLY feature_flag_overrides
    ADD CONSTRAINT feature_flag_overrides_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY feature_flag_rules
    ADD CONSTRAINT feature_flag_rules_flag_id_fkey FOREIGN KEY (flag_id) REFERENCES feature_flags(id) ON DELETE CASCADE;

ALTER TABLE ONLY feature_flags
    ADD CONSTRAINT feature_flags_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id);

ALTER TABLE ONLY feature_flags
    ADD CONSTRAINT feature_flags_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY issues
    ADD CONSTRAINT issues_assignee_id_fkey FOREIGN KEY (assignee_id) REFERENCES users(id);

ALTER TABLE ONLY issues
    ADD CONSTRAINT issues_author_id_fkey FOREIGN KEY (author_id) REFERENCES users(id);

ALTER TABLE ONLY issues
    ADD CONSTRAINT issues_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY llm_provider_configs
    ADD CONSTRAINT llm_provider_configs_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE log_entries
    ADD CONSTRAINT log_entries_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE log_entries
    ADD CONSTRAINT log_entries_session_id_fkey FOREIGN KEY (session_id) REFERENCES agent_sessions(id);

ALTER TABLE log_entries
    ADD CONSTRAINT log_entries_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id);

ALTER TABLE ONLY merge_requests
    ADD CONSTRAINT merge_requests_author_id_fkey FOREIGN KEY (author_id) REFERENCES users(id);

ALTER TABLE ONLY merge_requests
    ADD CONSTRAINT merge_requests_auto_merge_by_fkey FOREIGN KEY (auto_merge_by) REFERENCES users(id);

ALTER TABLE ONLY merge_requests
    ADD CONSTRAINT merge_requests_merged_by_fkey FOREIGN KEY (merged_by) REFERENCES users(id);

ALTER TABLE ONLY merge_requests
    ADD CONSTRAINT merge_requests_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY mesh_certs
    ADD CONSTRAINT mesh_certs_ca_id_fkey FOREIGN KEY (ca_id) REFERENCES mesh_ca(id) ON DELETE CASCADE;

ALTER TABLE metric_samples
    ADD CONSTRAINT metric_samples_series_id_fkey FOREIGN KEY (series_id) REFERENCES metric_series(id) ON DELETE CASCADE;

ALTER TABLE ONLY metric_series
    ADD CONSTRAINT metric_series_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id);

ALTER TABLE ONLY mr_reviews
    ADD CONSTRAINT mr_reviews_mr_id_fkey FOREIGN KEY (mr_id) REFERENCES merge_requests(id) ON DELETE CASCADE;

ALTER TABLE ONLY mr_reviews
    ADD CONSTRAINT mr_reviews_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY mr_reviews
    ADD CONSTRAINT mr_reviews_reviewer_id_fkey FOREIGN KEY (reviewer_id) REFERENCES users(id);

ALTER TABLE ONLY notifications
    ADD CONSTRAINT notifications_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY ops_repos
    ADD CONSTRAINT ops_repos_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY passkey_credentials
    ADD CONSTRAINT passkey_credentials_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY pipeline_steps
    ADD CONSTRAINT pipeline_steps_pipeline_id_fkey FOREIGN KEY (pipeline_id) REFERENCES pipelines(id) ON DELETE CASCADE;

ALTER TABLE ONLY pipeline_steps
    ADD CONSTRAINT pipeline_steps_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY pipelines
    ADD CONSTRAINT pipelines_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY pipelines
    ADD CONSTRAINT pipelines_triggered_by_fkey FOREIGN KEY (triggered_by) REFERENCES users(id);

ALTER TABLE ONLY platform_commands
    ADD CONSTRAINT platform_commands_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY platform_commands
    ADD CONSTRAINT platform_commands_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE;

ALTER TABLE ONLY projects
    ADD CONSTRAINT projects_owner_id_fkey FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY projects
    ADD CONSTRAINT projects_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES workspaces(id);

ALTER TABLE ONLY registry_blob_links
    ADD CONSTRAINT registry_blob_links_repository_id_fkey FOREIGN KEY (repository_id) REFERENCES registry_repositories(id) ON DELETE CASCADE;

ALTER TABLE ONLY registry_manifests
    ADD CONSTRAINT registry_manifests_repository_id_fkey FOREIGN KEY (repository_id) REFERENCES registry_repositories(id) ON DELETE CASCADE;

ALTER TABLE ONLY registry_repositories
    ADD CONSTRAINT registry_repositories_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY registry_tags
    ADD CONSTRAINT registry_tags_repository_id_fkey FOREIGN KEY (repository_id) REFERENCES registry_repositories(id) ON DELETE CASCADE;

ALTER TABLE ONLY release_assets
    ADD CONSTRAINT release_assets_release_id_fkey FOREIGN KEY (release_id) REFERENCES releases(id) ON DELETE CASCADE;

ALTER TABLE ONLY release_history
    ADD CONSTRAINT release_history_actor_id_fkey FOREIGN KEY (actor_id) REFERENCES users(id);

ALTER TABLE ONLY release_history
    ADD CONSTRAINT release_history_release_id_fkey FOREIGN KEY (release_id) REFERENCES deploy_releases(id) ON DELETE CASCADE;

ALTER TABLE ONLY release_history
    ADD CONSTRAINT release_history_target_id_fkey FOREIGN KEY (target_id) REFERENCES deploy_targets(id) ON DELETE CASCADE;

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id);

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY role_permissions
    ADD CONSTRAINT role_permissions_permission_id_fkey FOREIGN KEY (permission_id) REFERENCES permissions(id) ON DELETE CASCADE;

ALTER TABLE ONLY role_permissions
    ADD CONSTRAINT role_permissions_role_id_fkey FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE;

ALTER TABLE ONLY rollout_analyses
    ADD CONSTRAINT rollout_analyses_release_id_fkey FOREIGN KEY (release_id) REFERENCES deploy_releases(id) ON DELETE CASCADE;

ALTER TABLE ONLY secrets
    ADD CONSTRAINT secrets_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id);

ALTER TABLE ONLY secrets
    ADD CONSTRAINT secrets_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY secrets
    ADD CONSTRAINT secrets_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES workspaces(id);

ALTER TABLE spans
    ADD CONSTRAINT spans_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE spans
    ADD CONSTRAINT spans_session_id_fkey FOREIGN KEY (session_id) REFERENCES agent_sessions(id);

ALTER TABLE spans
    ADD CONSTRAINT spans_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id);

ALTER TABLE ONLY traces
    ADD CONSTRAINT traces_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY traces
    ADD CONSTRAINT traces_session_id_fkey FOREIGN KEY (session_id) REFERENCES agent_sessions(id);

ALTER TABLE ONLY traces
    ADD CONSTRAINT traces_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id);

ALTER TABLE ONLY user_gpg_keys
    ADD CONSTRAINT user_gpg_keys_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY user_provider_keys
    ADD CONSTRAINT user_provider_keys_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY user_roles
    ADD CONSTRAINT user_roles_granted_by_fkey FOREIGN KEY (granted_by) REFERENCES users(id);

ALTER TABLE ONLY user_roles
    ADD CONSTRAINT user_roles_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY user_roles
    ADD CONSTRAINT user_roles_role_id_fkey FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE;

ALTER TABLE ONLY user_roles
    ADD CONSTRAINT user_roles_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY user_ssh_keys
    ADD CONSTRAINT user_ssh_keys_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY webhooks
    ADD CONSTRAINT webhooks_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY workspace_members
    ADD CONSTRAINT workspace_members_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE ONLY workspace_members
    ADD CONSTRAINT workspace_members_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE;

ALTER TABLE ONLY workspaces
    ADD CONSTRAINT workspaces_owner_id_fkey FOREIGN KEY (owner_id) REFERENCES users(id);
