// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Integration tests for `pipeline::executor` — `gitops_sync` step type.
//!
//! These tests exercise the in-process `execute_gitops_sync_step()` and
//! `execute_gitops_sync_inner()` code paths (~275 lines) which were previously
//! untested. The `gitops_sync` step runs in-process (no K8s pod), reading from
//! the project's git repo and writing to an ops repo.

mod helpers;

use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Executor guard (same pattern as executor_integration.rs)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct ExecutorGuard {
    shutdown_tx: tokio::sync::watch::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

impl ExecutorGuard {
    fn spawn(state: &platform::store::AppState) -> Self {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
        let executor_state = state.clone();
        let handle = tokio::spawn(async move {
            platform::pipeline::executor::run(executor_state, shutdown_rx).await;
        });
        Self {
            shutdown_tx,
            handle,
        }
    }
}

// ---------------------------------------------------------------------------
// Gitops-sync specific setup helpers
// ---------------------------------------------------------------------------

const GITOPS_PLATFORM_YAML: &str = "\
pipeline:
  steps:
    - name: sync
      type: gitops_sync
      gitops:
        copy: [\"deploy/\", \".platform.yaml\"]
";

const DEPLOY_MANIFEST: &str = "\
apiVersion: apps/v1
kind: Deployment
metadata:
  name: app
spec:
  replicas: 1
  template:
    spec:
      containers:
        - name: app
          image: app:latest
";

/// Create a project with a bare git repo containing `.platform.yaml` and `deploy/`.
async fn setup_gitops_project(
    state: &platform::store::AppState,
    app: &axum::Router,
    token: &str,
    name: &str,
    platform_yaml: &str,
) -> (Uuid, PathBuf, PathBuf, tempfile::TempDir, tempfile::TempDir) {
    let project_id = helpers::create_project(app, token, name, "private").await;

    let (bare_dir, bare_path) = helpers::create_bare_repo();
    let (work_dir, work_path) = helpers::create_working_copy(&bare_path);

    std::fs::write(work_path.join(".platform.yaml"), platform_yaml).unwrap();
    std::fs::create_dir_all(work_path.join("deploy")).unwrap();
    std::fs::write(work_path.join("deploy/deployment.yaml"), DEPLOY_MANIFEST).unwrap();

    helpers::git_cmd(&work_path, &["add", "."]);
    helpers::git_cmd(
        &work_path,
        &["commit", "-m", "add platform config and deploy manifests"],
    );
    helpers::git_cmd(&work_path, &["push", "origin", "main"]);

    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    (project_id, bare_path, work_path, bare_dir, work_dir)
}

/// Create a bare ops repo and insert a DB entry in `ops_repos`.
async fn setup_ops_repo(
    state: &platform::store::AppState,
    project_id: Uuid,
    branch: &str,
) -> (Uuid, PathBuf, tempfile::TempDir) {
    let (ops_dir, ops_bare_path) = helpers::create_bare_repo();

    let (ops_work_dir, ops_work_path) = helpers::create_working_copy(&ops_bare_path);
    std::fs::write(ops_work_path.join("README.md"), "# Ops Repo\n").unwrap();
    helpers::git_cmd(&ops_work_path, &["add", "."]);
    helpers::git_cmd(&ops_work_path, &["commit", "-m", "initial ops commit"]);
    helpers::git_cmd(
        &ops_work_path,
        &["push", "origin", &format!("HEAD:refs/heads/{branch}")],
    );
    drop(ops_work_dir);

    let ops_repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO ops_repos (id, name, repo_path, branch, path, project_id)
         VALUES ($1, $2, $3, $4, '/', $5)",
    )
    .bind(ops_repo_id)
    .bind(format!("ops-{}", &project_id.to_string()[..8]))
    .bind(ops_bare_path.to_str().unwrap())
    .bind(branch)
    .bind(project_id)
    .execute(&state.pool)
    .await
    .unwrap();

    (ops_repo_id, ops_bare_path, ops_dir)
}

/// Insert a pipeline with a single `gitops_sync` step directly into the DB.
async fn insert_gitops_pipeline(
    state: &platform::store::AppState,
    project_id: Uuid,
    commit_sha: &str,
) -> (Uuid, Uuid) {
    let pipeline_id = Uuid::new_v4();
    let step_id = Uuid::new_v4();

    let admin_id: (Uuid,) = sqlx::query_as("SELECT id FROM users WHERE name = 'admin'")
        .fetch_one(&state.pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO pipelines (id, project_id, trigger, git_ref, commit_sha, status, triggered_by)
         VALUES ($1, $2, 'api', 'refs/heads/main', $3, 'pending', $4)",
    )
    .bind(pipeline_id)
    .bind(project_id)
    .bind(commit_sha)
    .bind(admin_id.0)
    .execute(&state.pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO pipeline_steps (id, pipeline_id, project_id, step_order, name, image, commands, step_type)
         VALUES ($1, $2, $3, 1, 'sync', '', '{}', 'gitops_sync')",
    )
    .bind(step_id)
    .bind(pipeline_id)
    .bind(project_id)
    .execute(&state.pool)
    .await
    .unwrap();

    (pipeline_id, step_id)
}

fn get_head_sha(bare_path: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["-C"])
        .arg(bare_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn get_branch_sha(bare_path: &std::path::Path, branch: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["-C"])
        .arg(bare_path)
        .args(["rev-parse", &format!("refs/heads/{branch}")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git rev-parse refs/heads/{branch} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn read_file_at_ref(bare_path: &std::path::Path, branch: &str, file_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(bare_path)
        .arg("show")
        .arg(format!("{branch}:{file_path}"))
        .output()
        .unwrap();
    if output.status.success() {
        Some(String::from_utf8(output.stdout).unwrap())
    } else {
        None
    }
}

/// Poll pipeline status until terminal.
async fn poll_pipeline_status(pool: &PgPool, pipeline_id: Uuid, timeout_secs: u64) -> String {
    let start = std::time::Instant::now();
    loop {
        let row: (String,) = sqlx::query_as("SELECT status FROM pipelines WHERE id = $1")
            .bind(pipeline_id)
            .fetch_one(pool)
            .await
            .unwrap();
        if matches!(row.0.as_str(), "success" | "failure" | "cancelled") {
            return row.0;
        }
        assert!(
            start.elapsed().as_secs() <= timeout_secs,
            "pipeline did not complete within {timeout_secs}s, last status: {}",
            row.0,
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// Poll a specific step's status until it is no longer pending/running.
async fn poll_step_status(
    pool: &PgPool,
    step_id: Uuid,
    timeout_secs: u64,
) -> (String, Option<i32>) {
    let start = std::time::Instant::now();
    loop {
        let row: (String, Option<i32>) =
            sqlx::query_as("SELECT status, exit_code FROM pipeline_steps WHERE id = $1")
                .bind(step_id)
                .fetch_one(pool)
                .await
                .unwrap();
        if matches!(row.0.as_str(), "success" | "failure" | "skipped") {
            return row;
        }
        assert!(
            start.elapsed().as_secs() <= timeout_secs,
            "step did not complete within {timeout_secs}s, last status: {}",
            row.0,
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

// ===========================================================================
// Test 1: Basic gitops_sync
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_gitops_sync_basic(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());
    let _executor = ExecutorGuard::spawn(&state);

    let (project_id, bare_path, _work_path, _bd, _wd) = setup_gitops_project(
        &state,
        &app,
        &admin_token,
        "gitops-basic",
        GITOPS_PLATFORM_YAML,
    )
    .await;

    let project_sha = get_head_sha(&bare_path);

    let (_ops_repo_id, ops_bare_path, _ops_dir) = setup_ops_repo(&state, project_id, "main").await;
    let ops_sha_before = get_branch_sha(&ops_bare_path, "main");

    let (pipeline_id, step_id) = insert_gitops_pipeline(&state, project_id, &project_sha).await;
    state.pipeline_notify.notify_one();

    let final_status = poll_pipeline_status(&state.pool, pipeline_id, 120).await;
    assert_eq!(
        final_status, "success",
        "gitops_sync pipeline should succeed"
    );

    let (step_status, exit_code) = poll_step_status(&state.pool, step_id, 5).await;
    assert_eq!(step_status, "success");
    assert_eq!(exit_code, Some(0));

    // Ops repo should have new commits
    let ops_sha_after = get_branch_sha(&ops_bare_path, "main");
    assert_ne!(ops_sha_before, ops_sha_after);

    // Verify values file
    let values_content = read_file_at_ref(&ops_bare_path, "main", "values/production.yaml");
    assert!(
        values_content.is_some(),
        "ops repo should contain values/production.yaml"
    );
    let values_str = values_content.unwrap();
    assert!(
        values_str.contains("image_ref"),
        "values should contain image_ref: {values_str}"
    );
    assert!(
        values_str.contains("gitops-basic"),
        "values should reference project name: {values_str}"
    );

    // platform.yaml + deploy/ manifests synced
    assert!(read_file_at_ref(&ops_bare_path, "main", "platform.yaml").is_some());
    assert!(read_file_at_ref(&ops_bare_path, "main", "deploy/deployment.yaml").is_some());
}

// ===========================================================================
// Test 2: gitops_sync with include_staging = true
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_gitops_sync_with_staging(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());
    let _executor = ExecutorGuard::spawn(&state);

    let (project_id, bare_path, _work_path, _bd, _wd) = setup_gitops_project(
        &state,
        &app,
        &admin_token,
        "gitops-stg",
        GITOPS_PLATFORM_YAML,
    )
    .await;

    let project_sha = get_head_sha(&bare_path);

    sqlx::query("UPDATE projects SET include_staging = true WHERE id = $1")
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let (_ops_repo_id, ops_bare_path, _ops_dir) = setup_ops_repo(&state, project_id, "main").await;

    let (pipeline_id, step_id) = insert_gitops_pipeline(&state, project_id, &project_sha).await;
    state.pipeline_notify.notify_one();

    let final_status = poll_pipeline_status(&state.pool, pipeline_id, 120).await;
    assert_eq!(final_status, "success");

    let (step_status, _) = poll_step_status(&state.pool, step_id, 5).await;
    assert_eq!(step_status, "success");

    // Values should be on "staging" branch
    let staging_values = read_file_at_ref(&ops_bare_path, "staging", "values/staging.yaml");
    assert!(
        staging_values.is_some(),
        "ops repo should have values/staging.yaml on staging branch"
    );
    let staging_str = staging_values.unwrap();
    assert!(
        staging_str.contains("\"environment\": \"staging\"")
            || staging_str.contains("environment: staging"),
        "staging values should reference staging environment: {staging_str}"
    );

    assert!(read_file_at_ref(&ops_bare_path, "staging", "platform.yaml").is_some());
}

// ===========================================================================
// Test 3: gitops_sync without ops repo — graceful failure
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_gitops_sync_no_ops_repo(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());
    let _executor = ExecutorGuard::spawn(&state);

    let (project_id, bare_path, _work_path, _bd, _wd) = setup_gitops_project(
        &state,
        &app,
        &admin_token,
        "gitops-noops",
        GITOPS_PLATFORM_YAML,
    )
    .await;

    let project_sha = get_head_sha(&bare_path);

    let (pipeline_id, step_id) = insert_gitops_pipeline(&state, project_id, &project_sha).await;
    state.pipeline_notify.notify_one();

    let final_status = poll_pipeline_status(&state.pool, pipeline_id, 120).await;
    assert_eq!(
        final_status, "failure",
        "pipeline should fail when no ops repo exists"
    );

    let (step_status, exit_code) = poll_step_status(&state.pool, step_id, 5).await;
    assert_eq!(step_status, "failure");
    assert_eq!(exit_code, Some(1));
}

// ===========================================================================
// Test 4: gitops_sync without .platform.yaml — should still sync deploy/
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_gitops_sync_no_platform_yaml(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());
    let _executor = ExecutorGuard::spawn(&state);

    let project_id = helpers::create_project(&app, &admin_token, "gitops-noyaml", "private").await;

    let (bare_dir, bare_path) = helpers::create_bare_repo();
    let (_work_dir, work_path) = helpers::create_working_copy(&bare_path);

    std::fs::create_dir_all(work_path.join("deploy")).unwrap();
    std::fs::write(
        work_path.join("deploy/service.yaml"),
        "apiVersion: v1\nkind: Service\n",
    )
    .unwrap();
    helpers::git_cmd(&work_path, &["add", "."]);
    helpers::git_cmd(&work_path, &["commit", "-m", "add deploy manifests only"]);
    helpers::git_cmd(&work_path, &["push", "origin", "main"]);

    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let project_sha = get_head_sha(&bare_path);

    let (_ops_repo_id, ops_bare_path, _ops_dir) = setup_ops_repo(&state, project_id, "main").await;

    let (pipeline_id, step_id) = insert_gitops_pipeline(&state, project_id, &project_sha).await;
    state.pipeline_notify.notify_one();

    let final_status = poll_pipeline_status(&state.pool, pipeline_id, 120).await;
    assert_eq!(
        final_status, "success",
        "should succeed without .platform.yaml"
    );

    let (step_status, exit_code) = poll_step_status(&state.pool, step_id, 5).await;
    assert_eq!(step_status, "success");
    assert_eq!(exit_code, Some(0));

    assert!(read_file_at_ref(&ops_bare_path, "main", "deploy/service.yaml").is_some());
    assert!(read_file_at_ref(&ops_bare_path, "main", "values/production.yaml").is_some());
    assert!(read_file_at_ref(&ops_bare_path, "main", "platform.yaml").is_none());

    drop(bare_dir);
}

// ===========================================================================
// Test 5: gitops_sync with feature flags
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_gitops_sync_with_feature_flags(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());
    let _executor = ExecutorGuard::spawn(&state);

    let platform_yaml_with_flags = "\
pipeline:
  steps:
    - name: sync
      type: gitops_sync
      gitops:
        copy: [\"deploy/\", \".platform.yaml\"]

flags:
  - key: enable_feature_x
    default_value: false
    description: \"Feature X toggle\"
  - key: max_retries
    default_value: 3
    description: \"Max retry count\"
";

    let (project_id, bare_path, _work_path, _bd, _wd) = setup_gitops_project(
        &state,
        &app,
        &admin_token,
        "gitops-flags",
        platform_yaml_with_flags,
    )
    .await;

    let project_sha = get_head_sha(&bare_path);

    let (_ops_repo_id, ops_bare_path, _ops_dir) = setup_ops_repo(&state, project_id, "main").await;

    let (pipeline_id, step_id) = insert_gitops_pipeline(&state, project_id, &project_sha).await;
    state.pipeline_notify.notify_one();

    let final_status = poll_pipeline_status(&state.pool, pipeline_id, 120).await;
    assert_eq!(final_status, "success");

    let (step_status, _) = poll_step_status(&state.pool, step_id, 5).await;
    assert_eq!(step_status, "success");

    let platform_yaml = read_file_at_ref(&ops_bare_path, "main", "platform.yaml");
    assert!(platform_yaml.is_some());
    let yaml_str = platform_yaml.unwrap();
    assert!(
        yaml_str.contains("enable_feature_x"),
        "should contain flags: {yaml_str}"
    );
    assert!(
        yaml_str.contains("max_retries"),
        "should contain all flags: {yaml_str}"
    );
}

// ===========================================================================
// Test 6: gitops_sync with per-environment variables
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_gitops_sync_with_variables(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());
    let _executor = ExecutorGuard::spawn(&state);

    let platform_yaml_with_vars = "\
pipeline:
  steps:
    - name: sync
      type: gitops_sync
      gitops:
        copy: [\"deploy/\", \".platform.yaml\"]

deploy:
  specs:
    - name: app
      type: rolling
  variables:
    production: deploy/variables_production.yaml
";

    let project_id = helpers::create_project(&app, &admin_token, "gitops-vars", "private").await;

    let (bare_dir, bare_path) = helpers::create_bare_repo();
    let (_work_dir, work_path) = helpers::create_working_copy(&bare_path);

    std::fs::write(work_path.join(".platform.yaml"), platform_yaml_with_vars).unwrap();
    std::fs::create_dir_all(work_path.join("deploy")).unwrap();
    std::fs::write(work_path.join("deploy/deployment.yaml"), DEPLOY_MANIFEST).unwrap();
    std::fs::write(
        work_path.join("deploy/variables_production.yaml"),
        "replicas: 3\nmemory_limit: 512Mi\ncpu_limit: 500m\ncustom_env: production-value\n",
    )
    .unwrap();

    helpers::git_cmd(&work_path, &["add", "."]);
    helpers::git_cmd(
        &work_path,
        &["commit", "-m", "add platform config with variables"],
    );
    helpers::git_cmd(&work_path, &["push", "origin", "main"]);

    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let project_sha = get_head_sha(&bare_path);

    let (_ops_repo_id, ops_bare_path, _ops_dir) = setup_ops_repo(&state, project_id, "main").await;

    let (pipeline_id, step_id) = insert_gitops_pipeline(&state, project_id, &project_sha).await;
    state.pipeline_notify.notify_one();

    let final_status = poll_pipeline_status(&state.pool, pipeline_id, 120).await;
    assert_eq!(final_status, "success");

    let (step_status, _) = poll_step_status(&state.pool, step_id, 5).await;
    assert_eq!(step_status, "success");

    let values_content = read_file_at_ref(&ops_bare_path, "main", "values/production.yaml");
    assert!(values_content.is_some());
    let values_str = values_content.unwrap();
    assert!(values_str.contains("image_ref"));
    assert!(
        values_str.contains("replicas"),
        "should contain merged variable: {values_str}"
    );
    assert!(
        values_str.contains("memory_limit"),
        "should contain merged variable: {values_str}"
    );
    assert!(
        values_str.contains("custom_env"),
        "should contain merged variable: {values_str}"
    );

    drop(bare_dir);
}
