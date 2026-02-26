mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

/// Create a project with a git repo and an initial unsigned commit.
async fn setup_project_with_commit(
    app: &axum::Router,
    admin_token: &str,
    state: &platform::store::AppState,
) -> (Uuid, String) {
    // Create a project
    let (status, body) = helpers::post_json(
        app,
        admin_token,
        "/api/projects",
        serde_json::json!({
            "name": "sig-test",
            "description": "Signature test project"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create project: {body}");
    let project_id: Uuid = body["id"].as_str().unwrap().parse().unwrap();

    // Get the repo path and create a bare repo with a commit
    let repo_dir = state
        .config
        .git_repos_path
        .join("admin")
        .join("sig-test.git");
    std::fs::create_dir_all(&repo_dir).unwrap();

    // Init bare repo
    let output = std::process::Command::new("git")
        .arg("init")
        .arg("--bare")
        .arg(&repo_dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "git init failed");

    // Create a temp working copy, commit, and push
    let work_dir = tempfile::tempdir().unwrap();
    let work_path = work_dir.path();

    std::process::Command::new("git")
        .arg("clone")
        .arg(&repo_dir)
        .arg(work_path)
        .output()
        .unwrap();

    // Configure git
    std::process::Command::new("git")
        .arg("-C")
        .arg(work_path)
        .args(["config", "user.email", "admin@localhost"])
        .output()
        .unwrap();
    std::process::Command::new("git")
        .arg("-C")
        .arg(work_path)
        .args(["config", "user.name", "Admin"])
        .output()
        .unwrap();

    // Create a file and commit
    std::fs::write(work_path.join("README.md"), "# Test\n").unwrap();
    std::process::Command::new("git")
        .arg("-C")
        .arg(work_path)
        .args(["add", "README.md"])
        .output()
        .unwrap();
    let commit_output = std::process::Command::new("git")
        .arg("-C")
        .arg(work_path)
        .args(["commit", "-m", "Initial commit"])
        .output()
        .unwrap();
    assert!(
        commit_output.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&commit_output.stderr)
    );

    // Get the SHA
    let sha_output = std::process::Command::new("git")
        .arg("-C")
        .arg(work_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let sha = String::from_utf8(sha_output.stdout)
        .unwrap()
        .trim()
        .to_owned();

    // Push to bare repo
    let push_output = std::process::Command::new("git")
        .arg("-C")
        .arg(work_path)
        .args(["push", "origin", "HEAD:refs/heads/main"])
        .output()
        .unwrap();
    assert!(
        push_output.status.success(),
        "git push failed: {}",
        String::from_utf8_lossy(&push_output.stderr)
    );

    (project_id, sha)
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_commits_without_verify_flag_no_signature(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (project_id, _sha) = setup_project_with_commit(&app, &admin_token, &state).await;

    // Fetch commits without verify_signatures flag
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/commits?ref=main"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let commits = body.as_array().unwrap();
    assert!(!commits.is_empty());
    // Without verify_signatures, signature field should be absent
    assert!(
        commits[0].get("signature").is_none(),
        "signature should be absent without verify flag"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_commits_with_verify_flag_unsigned(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (project_id, _sha) = setup_project_with_commit(&app, &admin_token, &state).await;

    // Fetch commits WITH verify_signatures flag
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/commits?ref=main&verify_signatures=true"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let commits = body.as_array().unwrap();
    assert!(!commits.is_empty());
    // Unsigned commit should have NoSignature status
    assert_eq!(commits[0]["signature"]["status"], "no_signature");
}

#[sqlx::test(migrations = "./migrations")]
async fn test_commit_detail_endpoint_unsigned(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (project_id, sha) = setup_project_with_commit(&app, &admin_token, &state).await;

    // Fetch single commit detail (always verifies signature)
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/commits/{sha}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sha"], sha);
    assert_eq!(body["signature"]["status"], "no_signature");
}

#[sqlx::test(migrations = "./migrations")]
async fn test_commit_detail_nonexistent_sha_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (project_id, _sha) = setup_project_with_commit(&app, &admin_token, &state).await;

    // Use a nonexistent SHA
    let fake_sha = "0000000000000000000000000000000000000000";
    let (status, _body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/commits/{fake_sha}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_commit_detail_unauthenticated_returns_401(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (project_id, sha) = setup_project_with_commit(&app, &admin_token, &state).await;

    // Use a bad token
    let (status, _body) = helpers::get_json(
        &app,
        "bad-token",
        &format!("/api/projects/{project_id}/commits/{sha}"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_commit_detail_invalid_sha_returns_400(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (project_id, _sha) = setup_project_with_commit(&app, &admin_token, &state).await;

    // Use an invalid SHA (too short)
    let (status, _body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/commits/abc"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_signature_cache_hit(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (project_id, sha) = setup_project_with_commit(&app, &admin_token, &state).await;

    // First call — computes and caches
    let (status, body1) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/commits/{sha}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body1}");
    assert_eq!(body1["signature"]["status"], "no_signature");

    // Verify cache key exists in Valkey
    use fred::interfaces::KeysInterface;
    let cache_key = format!("gpg:sig:{project_id}:{sha}");
    let exists: bool = state.valkey.exists(&cache_key).await.unwrap();
    assert!(exists, "cache key should exist after first verification");

    // Second call — should hit cache
    let (status, body2) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/commits/{sha}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body2}");
    assert_eq!(body2["signature"]["status"], "no_signature");
}
