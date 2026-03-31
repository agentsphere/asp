// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Additional integration tests for `src/api/merge_requests.rs` coverage gaps.
//!
//! Covers: delete MR, auto-merge enable/disable, MR on nonexistent project,
//! source branch validation, same-branch rejection, get/delete nonexistent MR/comment/review,
//! delete comment (author + admin + non-author), get single review, and MR body validation.

mod helpers;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

/// Get admin user ID from auth/me.
async fn get_admin_id(app: &axum::Router, token: &str) -> Uuid {
    let (_, body) = helpers::get_json(app, token, "/api/auth/me").await;
    Uuid::parse_str(body["id"].as_str().unwrap()).unwrap()
}

/// Insert an MR directly (bypassing branch checks).
async fn insert_mr(pool: &PgPool, project_id: Uuid, author_id: Uuid, number: i32) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r"INSERT INTO merge_requests (id, project_id, number, author_id, source_branch, target_branch, title, status)
          VALUES ($1, $2, $3, $4, 'feat', 'main', 'Test MR', 'open')",
    )
    .bind(id)
    .bind(project_id)
    .bind(number)
    .bind(author_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query("UPDATE projects SET next_mr_number = $1 WHERE id = $2")
        .bind(number + 1)
        .bind(project_id)
        .execute(pool)
        .await
        .unwrap();

    id
}

/// Insert a merged MR directly.
async fn insert_merged_mr(pool: &PgPool, project_id: Uuid, author_id: Uuid, number: i32) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r"INSERT INTO merge_requests (id, project_id, number, author_id, source_branch, target_branch, title, status, merged_by, merged_at)
          VALUES ($1, $2, $3, $4, 'feat-merged', 'main', 'Merged MR', 'merged', $4, now())",
    )
    .bind(id)
    .bind(project_id)
    .bind(number)
    .bind(author_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query("UPDATE projects SET next_mr_number = $1 WHERE id = $2")
        .bind(number + 1)
        .bind(project_id)
        .execute(pool)
        .await
        .unwrap();

    id
}

/// Insert a closed MR directly.
async fn insert_closed_mr(pool: &PgPool, project_id: Uuid, author_id: Uuid, number: i32) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r"INSERT INTO merge_requests (id, project_id, number, author_id, source_branch, target_branch, title, status)
          VALUES ($1, $2, $3, $4, 'feat-closed', 'main', 'Closed MR', 'closed')",
    )
    .bind(id)
    .bind(project_id)
    .bind(number)
    .bind(author_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query("UPDATE projects SET next_mr_number = $1 WHERE id = $2")
        .bind(number + 1)
        .bind(project_id)
        .execute(pool)
        .await
        .unwrap();

    id
}

// ---------------------------------------------------------------------------
// Delete MR tests
// ---------------------------------------------------------------------------

/// Deleting an open MR succeeds with 204.
#[sqlx::test(migrations = "./migrations")]
async fn delete_open_mr_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-mr-open", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's now closed
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "closed");
}

/// Deleting a merged MR fails with 409 Conflict.
#[sqlx::test(migrations = "./migrations")]
async fn delete_merged_mr_conflict(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-mr-merged", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_merged_mr(&pool, project_id, admin_id, 1).await;

    let (status, body) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("merged"));
}

/// Deleting an already-closed MR is idempotent (succeeds with 204).
#[sqlx::test(migrations = "./migrations")]
async fn delete_closed_mr_idempotent(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-mr-closed", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_closed_mr(&pool, project_id, admin_id, 1).await;

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// Deleting a nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn delete_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-mr-none", "public").await;

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Delete MR requires project:write permission.
#[sqlx::test(migrations = "./migrations")]
async fn delete_mr_requires_write(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-mr-perm", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    // Create a viewer
    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "mr-del-view", "mrdelview@test.com").await;
    helpers::assign_role(&app, &admin_token, user_id, "viewer", None, &pool).await;

    let (status, _) = helpers::delete_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/merge-requests/1"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Auto-merge enable/disable
// ---------------------------------------------------------------------------

/// Enable auto-merge on an open MR succeeds.
#[sqlx::test(migrations = "./migrations")]
async fn enable_auto_merge_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "auto-merge-en", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let (status, _) = helpers::put_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/auto-merge"),
        json!({"merge_method": "squash"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Verify the auto_merge flag was set in the DB
    let row: (bool,) = sqlx::query_as(
        "SELECT auto_merge FROM merge_requests WHERE project_id = $1 AND number = 1",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(row.0);
}

/// Enable auto-merge on nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn enable_auto_merge_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "auto-merge-404", "public").await;

    let (status, _) = helpers::put_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/auto-merge"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Enable auto-merge on closed MR returns 404 (WHERE status = 'open').
#[sqlx::test(migrations = "./migrations")]
async fn enable_auto_merge_closed_mr_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id =
        helpers::create_project(&app, &admin_token, "auto-merge-closed", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_closed_mr(&pool, project_id, admin_id, 1).await;

    let (status, _) = helpers::put_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/auto-merge"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Disable auto-merge succeeds.
#[sqlx::test(migrations = "./migrations")]
async fn disable_auto_merge_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "auto-merge-dis", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    // Enable first
    helpers::put_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/auto-merge"),
        json!({}),
    )
    .await;

    // Disable
    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/auto-merge"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Verify disabled
    let row: (bool,) = sqlx::query_as(
        "SELECT auto_merge FROM merge_requests WHERE project_id = $1 AND number = 1",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!row.0);
}

/// Disable auto-merge on nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn disable_auto_merge_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id =
        helpers::create_project(&app, &admin_token, "auto-merge-dis404", "public").await;

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/auto-merge"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Auto-merge requires project:write.
#[sqlx::test(migrations = "./migrations")]
async fn auto_merge_requires_write(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "auto-merge-perm", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "am-viewer", "amviewer@test.com").await;
    helpers::assign_role(&app, &admin_token, user_id, "viewer", None, &pool).await;

    let (status, _) = helpers::put_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/merge-requests/1/auto-merge"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// MR create validation edge cases
// ---------------------------------------------------------------------------

/// Creating MR with same source and target branch fails.
#[sqlx::test(migrations = "./migrations")]
async fn mr_same_source_target_branch_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "mr-same-br", "public").await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests"),
        json!({
            "source_branch": "main",
            "target_branch": "main",
            "title": "Same branch MR",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("differ"));
}

/// Creating MR with nonexistent source branch fails.
#[sqlx::test(migrations = "./migrations")]
async fn mr_nonexistent_source_branch_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "mr-no-branch", "public").await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests"),
        json!({
            "source_branch": "nonexistent-branch",
            "target_branch": "main",
            "title": "No branch MR",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("does not exist"));
}

/// Creating MR with body too long fails.
#[sqlx::test(migrations = "./migrations")]
async fn mr_body_too_long_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "mr-long-body", "public").await;

    let long_body = "x".repeat(100_001);
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests"),
        json!({
            "source_branch": "feat",
            "target_branch": "main",
            "title": "Long body MR",
            "body": long_body,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Creating MR with invalid branch name fails validation.
#[sqlx::test(migrations = "./migrations")]
async fn mr_invalid_branch_name_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "mr-bad-branch", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests"),
        json!({
            "source_branch": "feat/../evil",
            "target_branch": "main",
            "title": "Bad branch",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Creating MR with empty title fails validation.
#[sqlx::test(migrations = "./migrations")]
async fn mr_empty_title_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "mr-empty-title", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests"),
        json!({
            "source_branch": "feat",
            "target_branch": "main",
            "title": "",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Creating MR with title too long fails.
#[sqlx::test(migrations = "./migrations")]
async fn mr_title_too_long_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "mr-long-title", "public").await;

    let long_title = "t".repeat(501);
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests"),
        json!({
            "source_branch": "feat",
            "target_branch": "main",
            "title": long_title,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Get nonexistent MR / review / comment
// ---------------------------------------------------------------------------

/// Get nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn get_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "mr-get-404", "public").await;

    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Get review for nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn get_review_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rev-noexist-mr", "public").await;

    let fake_review = Uuid::new_v4();
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/reviews/{fake_review}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Get specific review by ID succeeds.
#[sqlx::test(migrations = "./migrations")]
async fn get_review_by_id_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rev-get-id", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    // Create a review
    let (_, review) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/reviews"),
        json!({ "verdict": "approve", "body": "Great work" }),
    )
    .await;
    let review_id = review["id"].as_str().unwrap();

    // Get the review by ID
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/reviews/{review_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["verdict"], "approve");
    assert_eq!(body["body"], "Great work");
}

/// Get nonexistent review by ID returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn get_nonexistent_review_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rev-404-id", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let fake_review_id = Uuid::new_v4();
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/reviews/{fake_review_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Delete comment tests
// ---------------------------------------------------------------------------

/// Delete own comment succeeds.
#[sqlx::test(migrations = "./migrations")]
async fn delete_own_comment_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-cmt-own", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let (_, comment) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments"),
        json!({ "body": "To be deleted" }),
    )
    .await;
    let comment_id = comment["id"].as_str().unwrap();

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments/{comment_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// Non-author non-admin cannot delete comment (forbidden).
#[sqlx::test(migrations = "./migrations")]
async fn delete_comment_non_author_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-cmt-forbid", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    // Admin creates a comment
    let (_, comment) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments"),
        json!({ "body": "Admin comment" }),
    )
    .await;
    let comment_id = comment["id"].as_str().unwrap();

    // Another user tries to delete
    let (_, user_token) =
        helpers::create_user(&app, &admin_token, "del-cmt-user", "delcmtuser@test.com").await;

    let (status, _) = helpers::delete_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments/{comment_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Delete nonexistent comment returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn delete_nonexistent_comment_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-cmt-404", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let fake_comment_id = Uuid::new_v4();
    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments/{fake_comment_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Delete comment on nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn delete_comment_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-cmt-nomr", "public").await;

    let fake_comment_id = Uuid::new_v4();
    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/comments/{fake_comment_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Get single comment by ID succeeds.
#[sqlx::test(migrations = "./migrations")]
async fn get_comment_by_id_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "get-cmt-id", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let (_, comment) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments"),
        json!({ "body": "Find me" }),
    )
    .await;
    let comment_id = comment["id"].as_str().unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments/{comment_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["body"], "Find me");
}

/// Get nonexistent comment by ID returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn get_nonexistent_comment_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "get-cmt-404", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let fake_id = Uuid::new_v4();
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments/{fake_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Get comment on nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn get_comment_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "get-cmt-nomr", "public").await;

    let fake_id = Uuid::new_v4();
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/comments/{fake_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Merge MR edge cases
// ---------------------------------------------------------------------------

/// Merging a nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn merge_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "merge-404", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/merge"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Merging a closed MR returns 400.
#[sqlx::test(migrations = "./migrations")]
async fn merge_closed_mr_returns_400(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "merge-closed", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_closed_mr(&pool, project_id, admin_id, 1).await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/merge"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("closed"));
}

/// Merging MR without project:write fails.
#[sqlx::test(migrations = "./migrations")]
async fn merge_mr_requires_write(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "merge-perm", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "merge-viewer", "mergeview@test.com").await;
    helpers::assign_role(&app, &admin_token, user_id, "viewer", None, &pool).await;

    let (status, _) = helpers::post_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/merge-requests/1/merge"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Update MR on nonexistent MR
// ---------------------------------------------------------------------------

/// Update nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn update_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "upd-mr-404", "public").await;

    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999"),
        json!({ "title": "New Title" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Review body too long is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn review_body_too_long_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rev-long-body", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let long_body = "x".repeat(100_001);
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/reviews"),
        json!({ "verdict": "comment", "body": long_body }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Review for nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn review_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rev-no-mr", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/reviews"),
        json!({ "verdict": "approve" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Comment for nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn comment_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "cmt-no-mr", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/comments"),
        json!({ "body": "Orphan comment" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// List comments for nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn list_comments_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "lst-cmt-nomr", "public").await;

    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/comments"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// List reviews for nonexistent MR returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn list_reviews_nonexistent_mr_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "lst-rev-nomr", "public").await;

    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/999/reviews"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Update MR title validation (too long).
#[sqlx::test(migrations = "./migrations")]
async fn update_mr_title_too_long(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "upd-mr-long", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let long_title = "t".repeat(501);
    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1"),
        json!({ "title": long_title }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Update MR body too long.
#[sqlx::test(migrations = "./migrations")]
async fn update_mr_body_too_long(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "upd-mr-lbody", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let long_body = "x".repeat(100_001);
    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1"),
        json!({ "body": long_body }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Update comment body validation (too long).
#[sqlx::test(migrations = "./migrations")]
async fn update_comment_body_too_long(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "upd-cmt-long", "public").await;
    let admin_id = get_admin_id(&app, &admin_token).await;
    insert_mr(&pool, project_id, admin_id, 1).await;

    let (_, comment) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments"),
        json!({ "body": "Original" }),
    )
    .await;
    let comment_id = comment["id"].as_str().unwrap();

    let long_body = "x".repeat(100_001);
    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/merge-requests/1/comments/{comment_id}"),
        json!({ "body": long_body }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
