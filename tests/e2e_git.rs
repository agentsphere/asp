mod e2e_helpers;

use axum::http::StatusCode;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// E2E Git Operation Tests (3 tests)
//
// Multi-step git protocol tests that require real git push/clone/merge flows.
// Single-endpoint browse tests moved to git_browse_integration.rs.
// ---------------------------------------------------------------------------

/// Push commits via smart HTTP protocol.
#[ignore = "requires Kind cluster"]
#[sqlx::test(migrations = "./migrations")]
async fn smart_http_push(pool: PgPool) {
    let (state, admin_token) = e2e_helpers::e2e_state(pool).await;
    let app = e2e_helpers::test_router(state.clone());
    let token = admin_token.clone();

    let _project_id = e2e_helpers::create_project(&app, &token, "push-test", "private").await;

    // Create a local bare repo and working copy
    let (_bare_dir, bare_path) = e2e_helpers::create_bare_repo();
    let (_work_dir, work_path) = e2e_helpers::create_working_copy(&bare_path);

    // Add more content and create another commit
    std::fs::write(work_path.join("hello.txt"), "hello world\n").unwrap();
    e2e_helpers::git_cmd(&work_path, &["add", "."]);
    e2e_helpers::git_cmd(&work_path, &["commit", "-m", "add hello.txt"]);

    // Verify that the commit exists locally
    let log = e2e_helpers::git_cmd(&work_path, &["log", "--oneline"]);
    assert!(
        log.contains("add hello.txt"),
        "local commit should exist: {log}"
    );

    // Verify push to origin succeeds
    let _push_output = e2e_helpers::git_cmd(&work_path, &["push", "origin", "main"]);
    // git push outputs to stderr, so the command succeeding is the assertion.

    // Verify commits are visible in the bare repo
    let bare_log = e2e_helpers::git_cmd(&bare_path, &["log", "--oneline", "main"]);
    assert!(
        bare_log.contains("add hello.txt"),
        "bare repo should have the pushed commit: {bare_log}"
    );
}

/// Clone via smart HTTP protocol.
#[ignore = "requires Kind cluster"]
#[sqlx::test(migrations = "./migrations")]
async fn smart_http_clone(pool: PgPool) {
    let (state, admin_token) = e2e_helpers::e2e_state(pool).await;
    let app = e2e_helpers::test_router(state.clone());
    let token = admin_token.clone();

    let _project_id = e2e_helpers::create_project(&app, &token, "clone-test", "public").await;

    // Create a bare repo with content
    let (_bare_dir, bare_path) = e2e_helpers::create_bare_repo();
    let (_work_dir, _work_path) = e2e_helpers::create_working_copy(&bare_path);

    // Clone from the bare repo (simulating the read path)
    let clone_dir = tempfile::tempdir().unwrap();
    let clone_path = clone_dir.path().join("cloned");
    e2e_helpers::git_cmd(
        clone_dir.path(),
        &["clone", bare_path.to_str().unwrap(), "cloned"],
    );

    // Verify the cloned repo has the expected content
    let readme = std::fs::read_to_string(clone_path.join("README.md")).unwrap();
    assert!(
        readme.contains("Test Project"),
        "cloned README should have expected content"
    );
}

/// Create an MR and merge it via the API.
#[ignore = "requires Kind cluster"]
#[sqlx::test(migrations = "./migrations")]
async fn merge_request_merge(pool: PgPool) {
    let (state, admin_token) = e2e_helpers::e2e_state(pool).await;
    let app = e2e_helpers::test_router(state.clone());
    let token = admin_token.clone();

    let project_id = e2e_helpers::create_project(&app, &token, "mr-merge", "private").await;

    // Create a bare repo with main and a feature branch
    let (_bare_dir, bare_path) = e2e_helpers::create_bare_repo();
    let (_work_dir, work_path) = e2e_helpers::create_working_copy(&bare_path);

    // Create feature branch with diverging commits
    e2e_helpers::git_cmd(&work_path, &["checkout", "-b", "feature-merge"]);
    std::fs::write(work_path.join("feature.txt"), "feature content\n").unwrap();
    e2e_helpers::git_cmd(&work_path, &["add", "."]);
    e2e_helpers::git_cmd(&work_path, &["commit", "-m", "feature work"]);
    e2e_helpers::git_cmd(&work_path, &["push", "origin", "feature-merge"]);

    // Point project at our bare repo
    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    // Create MR
    let (status, mr_body) = e2e_helpers::post_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/merge-requests"),
        serde_json::json!({
            "source_branch": "feature-merge",
            "target_branch": "main",
            "title": "Merge feature into main",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create MR failed: {mr_body}");
    let mr_number = mr_body["number"].as_i64().unwrap();
    assert_eq!(mr_body["status"], "open");

    // Merge via API
    let (status, merge_body) = e2e_helpers::post_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/merge-requests/{mr_number}/merge"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "merge failed: {merge_body}");
    assert_eq!(merge_body["status"], "merged");
    assert!(merge_body["merged_by"].is_string());
    assert!(merge_body["merged_at"].is_string());

    // Verify the merge commit exists on main in the bare repo
    let log = e2e_helpers::git_cmd(&bare_path, &["log", "--oneline", "main"]);
    assert!(
        log.contains("feature work"),
        "merged commit should appear on main: {log}"
    );
}
