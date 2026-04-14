// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Integration tests for API boundary validation edge cases.
//!
//! T7: Unicode, null bytes, special characters in text fields
//! T8: Pagination boundary values
//! T9: Rate limit boundary behavior
//! T25: Token expiry boundary values

mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// T7: Input validation — null bytes, special characters
// ---------------------------------------------------------------------------

/// Project name with null bytes is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn project_name_null_bytes(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/projects",
        serde_json::json!({ "name": "test\u{0000}evil", "visibility": "private" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Project name with emoji is rejected (check_name allows only alphanumeric + -_.).
#[sqlx::test(migrations = "./migrations")]
async fn project_name_unicode_emoji(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/projects",
        serde_json::json!({ "name": "project-🚀", "visibility": "public" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// SQL injection in project name is harmless (parameterized queries).
/// The name may be rejected by validation or stored safely.
#[sqlx::test(migrations = "./migrations")]
async fn project_name_sql_injection(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/projects",
        serde_json::json!({ "name": "'; DROP TABLE projects;--", "visibility": "private" }),
    )
    .await;
    // Should be rejected by check_name (contains quotes and semicolons)
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Issue title with null bytes is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn issue_title_null_bytes(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "null-issue-proj", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/issues"),
        serde_json::json!({ "title": "test\u{0000}issue" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// User name with special characters is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn user_name_special_chars(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users",
        serde_json::json!({
            "name": "admin'; --",
            "email": "sqli@test.com",
            "password": "testpass123"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// T8: Pagination boundary values
// ---------------------------------------------------------------------------

/// Pagination with limit=0 returns empty items (or defaults).
#[sqlx::test(migrations = "./migrations")]
async fn pagination_limit_zero(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    helpers::create_project(&app, &admin_token, "pag0-proj", "public").await;

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/projects?limit=0").await;
    assert_eq!(status, StatusCode::OK);
    // limit=0 is clamped to default or returns empty — either is acceptable
    let items = body["items"].as_array().unwrap();
    // just verify the response is valid JSON with items array
    assert!(items.len() <= 100);
}

/// Pagination with limit exceeding max is clamped to 100.
#[sqlx::test(migrations = "./migrations")]
async fn pagination_limit_exceeds_max(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/projects?limit=200").await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    assert!(items.len() <= 100, "limit should be clamped to max 100");
}

/// Pagination with negative limit — should return 400 or be treated as default.
#[sqlx::test(migrations = "./migrations")]
async fn pagination_negative_limit(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::get_json(&app, &admin_token, "/api/projects?limit=-1").await;
    // Negative i64 may deserialize fine but should be clamped or rejected
    assert!(
        status == StatusCode::OK || status == StatusCode::BAD_REQUEST,
        "negative limit should be handled gracefully, got {status}"
    );
}

/// Pagination with negative offset — should return 400 or be treated as 0.
#[sqlx::test(migrations = "./migrations")]
async fn pagination_negative_offset(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::get_json(&app, &admin_token, "/api/projects?offset=-1").await;
    assert!(
        status == StatusCode::OK || status == StatusCode::BAD_REQUEST,
        "negative offset should be handled gracefully, got {status}"
    );
}

/// Pagination with offset beyond total returns empty items.
#[sqlx::test(migrations = "./migrations")]
async fn pagination_offset_beyond_total(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/projects?offset=999999").await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        0,
        "offset beyond total should return empty items"
    );
}

// ---------------------------------------------------------------------------
// T25: Token expiry boundary values
// ---------------------------------------------------------------------------

/// Token with expires_in_days=0 should be rejected.
#[sqlx::test(migrations = "./migrations")]
async fn token_zero_days_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        serde_json::json!({ "name": "zero-day", "expires_in_days": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Token with negative expires_in_days should be rejected.
#[sqlx::test(migrations = "./migrations")]
async fn token_negative_days_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        serde_json::json!({ "name": "neg-day", "expires_in_days": -1 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Token with expires_in_days exceeding max (365) should be rejected.
#[sqlx::test(migrations = "./migrations")]
async fn token_exceeds_max_days_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        serde_json::json!({ "name": "over-max", "expires_in_days": 366 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Token at max boundary (365 days) should succeed.
#[sqlx::test(migrations = "./migrations")]
async fn token_at_max_days_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        serde_json::json!({ "name": "max-day", "expires_in_days": 365 }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "365-day token should succeed: {body}"
    );
}

/// Token at min boundary (1 day) should succeed.
#[sqlx::test(migrations = "./migrations")]
async fn token_at_min_days_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        serde_json::json!({ "name": "min-day", "expires_in_days": 1 }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "1-day token should succeed: {body}"
    );
}

// ---------------------------------------------------------------------------
// T15: Soft-deleted project resource access
// ---------------------------------------------------------------------------

/// Creating an issue on a deleted project returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn create_issue_on_deleted_project(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-issue-proj", "public").await;

    // Soft-delete the project
    let (status, _) =
        helpers::delete_json(&app, &admin_token, &format!("/api/projects/{project_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Try to create an issue on the deleted project
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/issues"),
        serde_json::json!({ "title": "ghost issue" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "should not create issue on deleted project"
    );
}

/// Triggering a pipeline on a deleted project returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn trigger_pipeline_on_deleted_project(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id =
        helpers::create_project(&app, &admin_token, "del-pipeline-proj", "public").await;

    // Soft-delete
    let (status, _) =
        helpers::delete_json(&app, &admin_token, &format!("/api/projects/{project_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Try to trigger pipeline
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/pipelines"),
        serde_json::json!({ "git_ref": "refs/heads/main" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "should not trigger pipeline on deleted project"
    );
}

/// Creating a webhook on a deleted project returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_on_deleted_project(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id =
        helpers::create_project(&app, &admin_token, "del-webhook-proj", "public").await;

    // Soft-delete
    let (status, _) =
        helpers::delete_json(&app, &admin_token, &format!("/api/projects/{project_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Try to create a webhook
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({ "url": "https://example.com/hook", "events": ["push"] }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "should not create webhook on deleted project"
    );
}

/// Listing projects excludes soft-deleted ones.
#[sqlx::test(migrations = "./migrations")]
async fn list_projects_excludes_deleted(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-list-proj", "public").await;

    // Count before delete
    let (_, before) = helpers::get_json(&app, &admin_token, "/api/projects").await;
    let before_total = before["total"].as_i64().unwrap();

    // Soft-delete
    helpers::delete_json(&app, &admin_token, &format!("/api/projects/{project_id}")).await;

    // Count after delete — should be one less
    let (_, after) = helpers::get_json(&app, &admin_token, "/api/projects").await;
    let after_total = after["total"].as_i64().unwrap();
    assert_eq!(
        after_total,
        before_total - 1,
        "deleted project should not appear in list"
    );
}

/// Creating a secret on a deleted project returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn create_secret_on_deleted_project(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "del-secret-proj", "public").await;

    // Soft-delete
    helpers::delete_json(&app, &admin_token, &format!("/api/projects/{project_id}")).await;

    // Try to create a secret
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/secrets"),
        serde_json::json!({ "name": "ghost-secret", "value": "nope", "scope": "agent" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "should not create secret on deleted project"
    );
}
