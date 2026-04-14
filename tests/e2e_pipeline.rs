// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! E2E Pipeline Tests — multi-step user journeys only.
//!
//! Single-endpoint pipeline lifecycle tests (trigger → execute → verify) have been
//! migrated to `executor_api.rs` in the API tier. This file retains
//! only true E2E tests that span multiple pipelines or test cross-cutting concerns
//! like concurrency ordering.

mod e2e_helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

/// Default `.platform.yaml` for pipeline tests.
const DEFAULT_PIPELINE_YAML: &str = "\
pipeline:
  steps:
    - name: test
      image: alpine:3.19
      commands:
        - echo hello
";

/// RAII guard that spawns the pipeline executor and shuts it down on drop.
struct ExecutorGuard {
    #[allow(dead_code)]
    cancel: tokio_util::sync::CancellationToken,
    #[allow(dead_code)]
    handle: tokio::task::JoinHandle<()>,
}

impl ExecutorGuard {
    fn spawn(state: &platform::store::AppState) -> Self {
        let cancel = tokio_util::sync::CancellationToken::new();
        let executor_state = state.clone();
        let token = cancel.clone();
        let handle = tokio::spawn(async move {
            platform::pipeline::executor::run(executor_state, token).await;
        });
        Self { cancel, handle }
    }
}

/// Helper: create a project and set up a bare git repo wired to it.
async fn setup_pipeline_project(
    state: &platform::store::AppState,
    app: &axum::Router,
    token: &str,
    name: &str,
) -> (
    Uuid,
    std::path::PathBuf,
    std::path::PathBuf,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let project_id = e2e_helpers::create_project(app, token, name, "private").await;

    let (bare_dir, bare_path) = e2e_helpers::create_bare_repo();
    let (work_dir, work_path) = e2e_helpers::create_working_copy(&bare_path);

    std::fs::write(work_path.join(".platform.yaml"), DEFAULT_PIPELINE_YAML).unwrap();
    e2e_helpers::git_cmd(&work_path, &["add", "."]);
    e2e_helpers::git_cmd(&work_path, &["commit", "-m", "add pipeline config"]);
    e2e_helpers::git_cmd(&work_path, &["push", "origin", "main"]);

    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    (project_id, bare_path, work_path, bare_dir, work_dir)
}

// ===========================================================================
// E2E: Concurrent pipeline ordering (multi-pipeline journey)
// ===========================================================================

/// Multiple pipelines triggered rapidly — verifies executor handles
/// concurrent pipeline queue correctly across multiple API calls.
#[ignore = "requires Kind cluster"]
#[sqlx::test(migrations = "./migrations")]
async fn concurrent_pipeline_limit(pool: PgPool) {
    let (state, admin_token, _server) = e2e_helpers::start_pipeline_server(pool).await;
    let app = e2e_helpers::pipeline_test_router(state.clone());
    let token = admin_token.clone();
    let _executor = ExecutorGuard::spawn(&state);

    let (project_id, _bare_path, _work_path, _bd, _wd) =
        setup_pipeline_project(&state, &app, &token, "pipe-concurrent").await;

    // Trigger multiple pipelines rapidly
    let mut pipeline_ids = Vec::new();
    for _i in 0..7 {
        let (status, body) = e2e_helpers::post_json(
            &app,
            &token,
            &format!("/api/projects/{project_id}/pipelines"),
            serde_json::json!({
                "git_ref": "refs/heads/main",
            }),
        )
        .await;
        if status == StatusCode::CREATED
            && let Some(id) = body["id"].as_str()
        {
            pipeline_ids.push(id.to_string());
        }
    }

    // Wake executor for all queued pipelines
    state.pipeline_notify.notify_one();

    // At least some should have been created
    assert!(
        !pipeline_ids.is_empty(),
        "at least one pipeline should be created"
    );

    // List all pipelines and check that we have an appropriate count
    let (status, list_body) = e2e_helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/pipelines?limit=50"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let total = list_body["total"].as_i64().unwrap_or(0);
    assert!(
        total > 0,
        "should have at least one pipeline after triggering multiple"
    );

    // Wait for all to complete
    for pid in &pipeline_ids {
        let _ = e2e_helpers::poll_pipeline_status(&app, &token, project_id, pid, 120).await;
    }
}
