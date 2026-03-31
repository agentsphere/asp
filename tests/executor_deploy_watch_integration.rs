//! Integration tests for `pipeline::executor` — `deploy_watch` step type.
//!
//! The `deploy_watch` step polls the `deploy_releases` table (joined with
//! `deploy_targets`) until it finds a terminal phase (`completed`, `failed`,
//! `rolled_back`, `cancelled`) or times out.

mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Executor guard + project setup (shared pattern with other executor tests)
// ---------------------------------------------------------------------------

/// RAII guard that spawns the pipeline executor and shuts it down on drop.
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

/// Create a project wired to a bare git repo with a custom `.platform.yaml`.
/// Returns `(project_id, bare_path, work_path, _bare_dir, _work_dir)`.
async fn setup_pipeline_project(
    state: &platform::store::AppState,
    app: &axum::Router,
    token: &str,
    name: &str,
    pipeline_yaml: &str,
) -> (Uuid, PathBuf, PathBuf, tempfile::TempDir, tempfile::TempDir) {
    let project_id = helpers::create_project(app, token, name, "private").await;

    let (bare_dir, bare_path) = helpers::create_bare_repo();
    let (work_dir, work_path) = helpers::create_working_copy(&bare_path);

    std::fs::write(work_path.join(".platform.yaml"), pipeline_yaml).unwrap();
    helpers::git_cmd(&work_path, &["add", "."]);
    helpers::git_cmd(&work_path, &["commit", "-m", "add pipeline config"]);
    helpers::git_cmd(&work_path, &["push", "origin", "main"]);

    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    (project_id, bare_path, work_path, bare_dir, work_dir)
}

/// Trigger a pipeline via the API and return `(pipeline_id_str, body)`.
async fn trigger_pipeline(
    app: &axum::Router,
    token: &str,
    project_id: Uuid,
) -> (String, serde_json::Value) {
    let (status, body) = helpers::post_json(
        app,
        token,
        &format!("/api/projects/{project_id}/pipelines"),
        serde_json::json!({ "git_ref": "refs/heads/main" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "trigger failed: {body}");
    let pipeline_id = body["id"]
        .as_str()
        .expect("pipeline should have id")
        .to_string();
    (pipeline_id, body)
}

/// Insert a `deploy_targets` row and return its ID.
async fn insert_deploy_target(pool: &PgPool, project_id: Uuid, environment: &str) -> Uuid {
    let target_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deploy_targets (id, project_id, name, environment)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(target_id)
    .bind(project_id)
    .bind(format!("{environment}-target"))
    .bind(environment)
    .execute(pool)
    .await
    .unwrap();
    target_id
}

/// Insert a `deploy_releases` row with the given phase.
async fn insert_deploy_release(
    pool: &PgPool,
    project_id: Uuid,
    target_id: Uuid,
    phase: &str,
) -> Uuid {
    let release_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deploy_releases (id, target_id, project_id, image_ref, phase)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(release_id)
    .bind(target_id)
    .bind(project_id)
    .bind("registry.local/app:v1")
    .bind(phase)
    .execute(pool)
    .await
    .unwrap();
    release_id
}

// ===========================================================================
// Test 1: deploy_watch with completed release succeeds
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_deploy_watch_completed(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());

    let yaml = "\
pipeline:
  steps:
    - name: watch-deploy
      type: deploy_watch
      deploy_watch:
        environment: staging
        timeout: 60
";

    let (project_id, _bare, _work, _bd, _wd) =
        setup_pipeline_project(&state, &app, &admin_token, "dw-ok", yaml).await;

    // Pre-insert a completed release so the watch resolves immediately
    let target_id = insert_deploy_target(&state.pool, project_id, "staging").await;
    insert_deploy_release(&state.pool, project_id, target_id, "completed").await;

    let (pipeline_id, _) = trigger_pipeline(&app, &admin_token, project_id).await;
    let _executor = ExecutorGuard::spawn(&state);
    state.pipeline_notify.notify_one();

    let final_status =
        helpers::poll_pipeline_status(&app, &admin_token, project_id, &pipeline_id, 60).await;
    assert_eq!(final_status, "success");

    // Verify step-level detail
    let (_, detail) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/pipelines/{pipeline_id}"),
    )
    .await;
    let steps = detail["steps"].as_array().expect("should have steps");
    assert_eq!(steps[0]["status"].as_str(), Some("success"));
    assert_eq!(steps[0]["exit_code"].as_i64(), Some(0));
    assert!(steps[0]["duration_ms"].as_i64().is_some());
}

// ===========================================================================
// Test 2: deploy_watch with failed release fails
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_deploy_watch_failed_release(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());

    let yaml = "\
pipeline:
  steps:
    - name: watch-deploy
      type: deploy_watch
      deploy_watch:
        environment: staging
        timeout: 60
";

    let (project_id, _bare, _work, _bd, _wd) =
        setup_pipeline_project(&state, &app, &admin_token, "dw-fail", yaml).await;

    let target_id = insert_deploy_target(&state.pool, project_id, "staging").await;
    insert_deploy_release(&state.pool, project_id, target_id, "failed").await;

    let (pipeline_id, _) = trigger_pipeline(&app, &admin_token, project_id).await;
    let _executor = ExecutorGuard::spawn(&state);
    state.pipeline_notify.notify_one();

    let final_status =
        helpers::poll_pipeline_status(&app, &admin_token, project_id, &pipeline_id, 60).await;
    assert_eq!(final_status, "failure");

    let (_, detail) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/pipelines/{pipeline_id}"),
    )
    .await;
    let steps = detail["steps"].as_array().expect("should have steps");
    assert_eq!(steps[0]["status"].as_str(), Some("failure"));
    assert_eq!(steps[0]["exit_code"].as_i64(), Some(1));
}

// ===========================================================================
// Test 3: deploy_watch with rolled_back release fails
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_deploy_watch_rolled_back(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());

    let yaml = "\
pipeline:
  steps:
    - name: watch-deploy
      type: deploy_watch
      deploy_watch:
        environment: staging
        timeout: 60
";

    let (project_id, _bare, _work, _bd, _wd) =
        setup_pipeline_project(&state, &app, &admin_token, "dw-rb", yaml).await;

    let target_id = insert_deploy_target(&state.pool, project_id, "staging").await;
    insert_deploy_release(&state.pool, project_id, target_id, "rolled_back").await;

    let (pipeline_id, _) = trigger_pipeline(&app, &admin_token, project_id).await;
    let _executor = ExecutorGuard::spawn(&state);
    state.pipeline_notify.notify_one();

    let final_status =
        helpers::poll_pipeline_status(&app, &admin_token, project_id, &pipeline_id, 60).await;
    assert_eq!(final_status, "failure");

    let (_, detail) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/pipelines/{pipeline_id}"),
    )
    .await;
    let steps = detail["steps"].as_array().expect("should have steps");
    assert_eq!(steps[0]["status"].as_str(), Some("failure"));
    assert_eq!(steps[0]["exit_code"].as_i64(), Some(1));
}

// ===========================================================================
// Test 4: deploy_watch times out when no release exists
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_deploy_watch_timeout(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());

    // Timeout = 1s. The loop sleeps 5s between polls, so after the first
    // unsuccessful poll it will exceed the deadline and break with failure.
    let yaml = "\
pipeline:
  steps:
    - name: watch-deploy
      type: deploy_watch
      deploy_watch:
        environment: staging
        timeout: 1
";

    let (project_id, _bare, _work, _bd, _wd) =
        setup_pipeline_project(&state, &app, &admin_token, "dw-timeout", yaml).await;

    // Deliberately NO deploy_targets or deploy_releases

    let (pipeline_id, _) = trigger_pipeline(&app, &admin_token, project_id).await;
    let _executor = ExecutorGuard::spawn(&state);
    state.pipeline_notify.notify_one();

    let final_status =
        helpers::poll_pipeline_status(&app, &admin_token, project_id, &pipeline_id, 60).await;
    assert_eq!(final_status, "failure");

    let (_, detail) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/pipelines/{pipeline_id}"),
    )
    .await;
    let steps = detail["steps"].as_array().expect("should have steps");
    assert_eq!(steps[0]["status"].as_str(), Some("failure"));
    assert_eq!(steps[0]["exit_code"].as_i64(), Some(1));
}

// ===========================================================================
// Test 5: deploy_watch with delayed completion (progressing -> completed)
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_deploy_watch_delayed_completion(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());

    let yaml = "\
pipeline:
  steps:
    - name: watch-deploy
      type: deploy_watch
      deploy_watch:
        environment: staging
        timeout: 60
";

    let (project_id, _bare, _work, _bd, _wd) =
        setup_pipeline_project(&state, &app, &admin_token, "dw-delay", yaml).await;

    // Start with progressing (non-terminal)
    let target_id = insert_deploy_target(&state.pool, project_id, "staging").await;
    let release_id = insert_deploy_release(&state.pool, project_id, target_id, "progressing").await;

    let (pipeline_id, _) = trigger_pipeline(&app, &admin_token, project_id).await;
    let _executor = ExecutorGuard::spawn(&state);
    state.pipeline_notify.notify_one();

    // Background task flips release to completed after 2s
    let pool_bg = state.pool.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        sqlx::query("UPDATE deploy_releases SET phase = 'completed' WHERE id = $1")
            .bind(release_id)
            .execute(&pool_bg)
            .await
            .unwrap();
    });

    let final_status =
        helpers::poll_pipeline_status(&app, &admin_token, project_id, &pipeline_id, 60).await;
    assert_eq!(final_status, "success");

    let (_, detail) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/pipelines/{pipeline_id}"),
    )
    .await;
    let steps = detail["steps"].as_array().expect("should have steps");
    assert_eq!(steps[0]["status"].as_str(), Some("success"));
    assert_eq!(steps[0]["exit_code"].as_i64(), Some(0));
    assert!(steps[0]["duration_ms"].as_i64().unwrap_or(0) > 0);
}

// ===========================================================================
// Test 6: deploy_watch with NULL step_config defaults to staging / 300s
// ===========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn executor_deploy_watch_default_config(pool: PgPool) {
    let (state, admin_token, _server) = helpers::start_pipeline_server(pool).await;
    let app = helpers::test_router(state.clone());

    let yaml = "\
pipeline:
  steps:
    - name: watch-deploy
      type: deploy_watch
      deploy_watch:
        environment: staging
";

    let (project_id, _bare, _work, _bd, _wd) =
        setup_pipeline_project(&state, &app, &admin_token, "dw-default", yaml).await;

    // Pre-insert a completed release for "staging" (the default environment)
    let target_id = insert_deploy_target(&state.pool, project_id, "staging").await;
    insert_deploy_release(&state.pool, project_id, target_id, "completed").await;

    let (pipeline_id, _) = trigger_pipeline(&app, &admin_token, project_id).await;

    // Clear step_config to force the default path
    let pipeline_uuid = Uuid::parse_str(&pipeline_id).unwrap();
    sqlx::query(
        "UPDATE pipeline_steps SET step_config = NULL \
         WHERE pipeline_id = $1 AND step_type = 'deploy_watch'",
    )
    .bind(pipeline_uuid)
    .execute(&state.pool)
    .await
    .unwrap();

    let _executor = ExecutorGuard::spawn(&state);
    state.pipeline_notify.notify_one();

    let final_status =
        helpers::poll_pipeline_status(&app, &admin_token, project_id, &pipeline_id, 60).await;
    assert_eq!(final_status, "success");

    let (_, detail) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/pipelines/{pipeline_id}"),
    )
    .await;
    let steps = detail["steps"].as_array().expect("should have steps");
    assert_eq!(steps[0]["status"].as_str(), Some("success"));
    assert_eq!(steps[0]["exit_code"].as_i64(), Some(0));
}
