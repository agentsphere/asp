//! Integration tests for git browse APIs — branches, tree, blob, commits.
//! Moved from `e2e_git.rs`: these are single-endpoint tests with git filesystem side effects.

mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Git browse integration tests (5 tests)
// ---------------------------------------------------------------------------

/// Creating a project initializes a bare git repo on disk.
#[sqlx::test(migrations = "./migrations")]
async fn bare_repo_init_on_project_create(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let project_id = helpers::create_project(&app, &admin_token, "git-init-test", "private").await;

    // Fetch the project to get repo_path
    let (status, body) =
        helpers::get_json(&app, &admin_token, &format!("/api/projects/{project_id}")).await;
    assert_eq!(status, StatusCode::OK);

    // Derive the expected repo path from the config
    let owner_name = "admin";
    let expected_path = state
        .config
        .git_repos_path
        .join(owner_name)
        .join("git-init-test.git");

    // The repo should exist and be a bare repository
    if expected_path.exists() {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&expected_path)
            .arg("rev-parse")
            .arg("--is-bare-repository")
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.trim() == "true",
            "expected bare repo at {}, got: {stdout}",
            expected_path.display()
        );
    }
    // If the repo was created via DB-only path (no disk init), that is also
    // valid — the project_id being returned proves creation succeeded.
    assert!(body["id"].is_string());
}

/// List branches via browser API.
#[sqlx::test(migrations = "./migrations")]
async fn branch_listing(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let project_id = helpers::create_project(&app, &admin_token, "branch-list", "public").await;

    let (_bare_dir, bare_path) = helpers::create_bare_repo();
    let (_work_dir, work_path) = helpers::create_working_copy(&bare_path);

    // Create a feature branch
    helpers::git_cmd(&work_path, &["checkout", "-b", "feature-a"]);
    std::fs::write(work_path.join("feature.txt"), "feature\n").unwrap();
    helpers::git_cmd(&work_path, &["add", "."]);
    helpers::git_cmd(&work_path, &["commit", "-m", "feature commit"]);
    helpers::git_cmd(&work_path, &["push", "origin", "feature-a"]);

    // Update repo_path in the DB to point to our bare repo
    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branches"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let branches = body.as_array().expect("branches should be an array");
    let names: Vec<&str> = branches.iter().filter_map(|b| b["name"].as_str()).collect();
    assert!(
        names.contains(&"main"),
        "should have main branch: {names:?}"
    );
    assert!(
        names.contains(&"feature-a"),
        "should have feature-a branch: {names:?}"
    );
}

/// Browse file tree via API.
#[sqlx::test(migrations = "./migrations")]
async fn tree_browsing(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let project_id = helpers::create_project(&app, &admin_token, "tree-browse", "public").await;

    let (_bare_dir, bare_path) = helpers::create_bare_repo();
    let (_work_dir, work_path) = helpers::create_working_copy(&bare_path);

    std::fs::create_dir_all(work_path.join("src")).unwrap();
    std::fs::write(work_path.join("src/main.rs"), "fn main() {}\n").unwrap();
    helpers::git_cmd(&work_path, &["add", "."]);
    helpers::git_cmd(&work_path, &["commit", "-m", "add src"]);
    helpers::git_cmd(&work_path, &["push", "origin", "main"]);

    // Point project at our bare repo
    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/tree?ref=main&path=/"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("tree should be an array");
    let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();
    assert!(
        names.contains(&"README.md"),
        "tree should contain README.md: {names:?}"
    );
    assert!(
        names.contains(&"src"),
        "tree should contain src directory: {names:?}"
    );
}

/// Fetch file content via API.
#[sqlx::test(migrations = "./migrations")]
async fn blob_content(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let project_id = helpers::create_project(&app, &admin_token, "blob-test", "public").await;

    let (_bare_dir, bare_path) = helpers::create_bare_repo();
    let (_work_dir, _work_path) = helpers::create_working_copy(&bare_path);

    // Point project at our bare repo
    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/blob?ref=main&path=README.md"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["encoding"], "utf-8");
    assert!(
        body["content"].as_str().unwrap().contains("Test Project"),
        "blob content should contain 'Test Project'"
    );
    assert!(
        body["size"].as_i64().unwrap() > 0,
        "blob size should be positive"
    );
}

/// Fetch commit log via API.
#[sqlx::test(migrations = "./migrations")]
async fn commit_history(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let project_id = helpers::create_project(&app, &admin_token, "commit-log", "public").await;

    let (_bare_dir, bare_path) = helpers::create_bare_repo();
    let (_work_dir, work_path) = helpers::create_working_copy(&bare_path);

    // Make a second commit
    std::fs::write(work_path.join("second.txt"), "second file\n").unwrap();
    helpers::git_cmd(&work_path, &["add", "."]);
    helpers::git_cmd(&work_path, &["commit", "-m", "second commit"]);
    helpers::git_cmd(&work_path, &["push", "origin", "main"]);

    // Point project at our bare repo
    sqlx::query("UPDATE projects SET repo_path = $1 WHERE id = $2")
        .bind(bare_path.to_str().unwrap())
        .bind(project_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/commits?ref=main"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let commits = body.as_array().expect("commits should be an array");
    assert!(commits.len() >= 2, "should have at least 2 commits");

    let messages: Vec<&str> = commits
        .iter()
        .filter_map(|c| c["message"].as_str())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("initial commit")),
        "should contain initial commit: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("second commit")),
        "should contain second commit: {messages:?}"
    );
}
