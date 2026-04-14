// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Additional integration tests for `src/api/users.rs` coverage gaps.
//!
//! Covers: create agent user type, update user password verification,
//! create user validation errors, API token scope validation, API token
//! expiry limits, deactivate nonexistent user, get/revoke tokens.

mod helpers;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Create user edge cases
// ---------------------------------------------------------------------------

/// Create user with agent type (no password required).
#[sqlx::test(migrations = "./migrations")]
async fn create_agent_user_no_password(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users",
        json!({
            "name": "agent-user-1",
            "email": "agent1@test.com",
            "user_type": "agent",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create agent failed: {body}");
    assert_eq!(body["user_type"], "agent");
}

/// Create agent user with password is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn create_agent_user_with_password_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users",
        json!({
            "name": "agent-user-pw",
            "email": "agentpw@test.com",
            "user_type": "agent",
            "password": "shouldfail123",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("must not be provided")
    );
}

/// Create human user without password is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn create_human_user_no_password_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users",
        json!({
            "name": "human-nopw",
            "email": "nopw@test.com",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("password is required")
    );
}

/// Create user with password too short is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn create_user_short_password_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users",
        json!({
            "name": "shortpw",
            "email": "shortpw@test.com",
            "password": "short",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Create user with invalid email is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn create_user_invalid_email_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users",
        json!({
            "name": "bademail",
            "email": "not-an-email",
            "password": "testpass123",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Create user with display_name.
#[sqlx::test(migrations = "./migrations")]
async fn create_user_with_display_name(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users",
        json!({
            "name": "display-user",
            "email": "display@test.com",
            "password": "testpass123",
            "display_name": "Display Name",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["display_name"], "Display Name");
}

// ---------------------------------------------------------------------------
// Update user edge cases
// ---------------------------------------------------------------------------

/// User changing own password must provide current_password.
#[sqlx::test(migrations = "./migrations")]
async fn update_own_password_requires_current(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "pw-user", "pwuser@test.com").await;

    // Try to change password without current_password
    let (status, body) = helpers::patch_json(
        &app,
        &user_token,
        &format!("/api/users/{user_id}"),
        json!({ "password": "newpass12345" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("current_password"));
}

/// User providing wrong current_password is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn update_own_password_wrong_current_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "pw-wrong", "pwwrong@test.com").await;

    let (status, body) = helpers::patch_json(
        &app,
        &user_token,
        &format!("/api/users/{user_id}"),
        json!({
            "password": "newpass12345",
            "current_password": "wrongpassword",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("incorrect"));
}

/// User can change own password with correct current_password.
#[sqlx::test(migrations = "./migrations")]
async fn update_own_password_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "pw-ok", "pwok@test.com").await;

    let (status, _) = helpers::patch_json(
        &app,
        &user_token,
        &format!("/api/users/{user_id}"),
        json!({
            "password": "newpass12345",
            "current_password": "testpass123",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Login with the new password should work
    let (status, _) = helpers::post_json(
        &app,
        "",
        "/api/auth/login",
        json!({ "name": "pw-ok", "password": "newpass12345" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Admin can change other user's password without current_password.
#[sqlx::test(migrations = "./migrations")]
async fn admin_can_change_others_password(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (user_id, _user_token) =
        helpers::create_user(&app, &admin_token, "pw-other", "pwother@test.com").await;

    // Admin changes user password (no current_password needed)
    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/users/{user_id}"),
        json!({ "password": "adminsetpass12" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Update nonexistent user returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn update_nonexistent_user_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let fake_id = Uuid::new_v4();
    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/users/{fake_id}"),
        json!({ "display_name": "Ghost" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Non-admin cannot update other user.
#[sqlx::test(migrations = "./migrations")]
async fn update_other_user_non_admin_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (other_id, _) =
        helpers::create_user(&app, &admin_token, "upd-target", "updtarget@test.com").await;
    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "upd-attacker", "updattacker@test.com").await;

    let (status, _) = helpers::patch_json(
        &app,
        &user_token,
        &format!("/api/users/{other_id}"),
        json!({ "display_name": "Hacked" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Deactivate user edge cases
// ---------------------------------------------------------------------------

/// Deactivate nonexistent user returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn deactivate_nonexistent_user_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let fake_id = Uuid::new_v4();
    let (status, _) =
        helpers::delete_json(&app, &admin_token, &format!("/api/users/{fake_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Deactivating a user revokes their sessions and tokens.
#[sqlx::test(migrations = "./migrations")]
async fn deactivate_user_revokes_auth(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let (user_id, _user_token) =
        helpers::create_user(&app, &admin_token, "deact-user", "deactuser@test.com").await;

    // Deactivate
    let (status, _) =
        helpers::delete_json(&app, &admin_token, &format!("/api/users/{user_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify tokens and sessions are cleared
    let sessions: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM auth_sessions WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions.0, 0);
}

// ---------------------------------------------------------------------------
// API Token edge cases
// ---------------------------------------------------------------------------

/// Token with invalid expiry (0 days) is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn create_token_invalid_expiry_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        json!({
            "name": "bad-expiry",
            "expires_in_days": 0,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("expires_in_days"));
}

/// Token with expiry too long is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn create_token_expiry_too_long_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        json!({
            "name": "long-expiry",
            "expires_in_days": 999,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Token with unknown scope is rejected.
#[sqlx::test(migrations = "./migrations")]
async fn create_token_unknown_scope_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        json!({
            "name": "bad-scope",
            "scopes": ["nonexistent:scope"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("unknown scope"));
}

/// Token with wildcard scope (*) is allowed.
#[sqlx::test(migrations = "./migrations")]
async fn create_token_wildcard_scope_allowed(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        json!({
            "name": "wildcard",
            "scopes": ["*"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "wildcard failed: {body}");
    assert!(body["token"].is_string());
}

/// Get specific token by ID.
#[sqlx::test(migrations = "./migrations")]
async fn get_api_token_by_id(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    // Create a token
    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        json!({ "name": "test-token" }),
    )
    .await;
    let token_id = create_body["id"].as_str().unwrap();

    // Get by ID
    let (status, body) =
        helpers::get_json(&app, &admin_token, &format!("/api/tokens/{token_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "test-token");
}

/// Get nonexistent token returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn get_nonexistent_token_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let fake_id = Uuid::new_v4();
    let (status, _) =
        helpers::get_json(&app, &admin_token, &format!("/api/tokens/{fake_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Revoke (delete) a token by ID.
#[sqlx::test(migrations = "./migrations")]
async fn revoke_api_token_succeeds(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/tokens",
        json!({ "name": "revoke-me" }),
    )
    .await;
    let token_id = create_body["id"].as_str().unwrap();

    let (status, _) =
        helpers::delete_json(&app, &admin_token, &format!("/api/tokens/{token_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) =
        helpers::get_json(&app, &admin_token, &format!("/api/tokens/{token_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Revoke nonexistent token returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn revoke_nonexistent_token_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let fake_id = Uuid::new_v4();
    let (status, _) =
        helpers::delete_json(&app, &admin_token, &format!("/api/tokens/{fake_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Login edge cases
// ---------------------------------------------------------------------------

/// Login with nonexistent user returns 401 (timing-safe).
#[sqlx::test(migrations = "./migrations")]
async fn login_nonexistent_user_returns_401(pool: PgPool) {
    let (state, _admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        "",
        "/api/auth/login",
        json!({ "name": "nobody", "password": "wrongpass123" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Login with wrong password returns 401.
#[sqlx::test(migrations = "./migrations")]
async fn login_wrong_password_returns_401(pool: PgPool) {
    let (state, _admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        "",
        "/api/auth/login",
        json!({ "name": "admin", "password": "wrongpassword" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Login with deactivated user returns 401.
#[sqlx::test(migrations = "./migrations")]
async fn login_deactivated_user_returns_401(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (user_id, _) =
        helpers::create_user(&app, &admin_token, "deact-login", "deactlogin@test.com").await;

    // Deactivate
    helpers::delete_json(&app, &admin_token, &format!("/api/users/{user_id}")).await;

    // Try to login
    let (status, _) = helpers::post_json(
        &app,
        "",
        "/api/auth/login",
        json!({ "name": "deact-login", "password": "testpass123" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Get user self (non-admin can read own profile).
#[sqlx::test(migrations = "./migrations")]
async fn get_own_user_profile(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "self-read", "selfread@test.com").await;

    let (status, body) =
        helpers::get_json(&app, &user_token, &format!("/api/users/{user_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "self-read");
}

/// Non-admin cannot get other user's profile.
#[sqlx::test(migrations = "./migrations")]
async fn get_other_user_non_admin_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (other_id, _) =
        helpers::create_user(&app, &admin_token, "other-prof", "otherprof@test.com").await;
    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "peeker", "peeker@test.com").await;

    let (status, _) = helpers::get_json(&app, &user_token, &format!("/api/users/{other_id}")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Get nonexistent user returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn get_nonexistent_user_returns_404(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let fake_id = Uuid::new_v4();
    let (status, _) = helpers::get_json(&app, &admin_token, &format!("/api/users/{fake_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Non-admin cannot list users.
#[sqlx::test(migrations = "./migrations")]
async fn list_users_non_admin_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "list-noadm", "listnoadm@test.com").await;

    let (status, _) = helpers::get_json(&app, &user_token, "/api/users").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Non-admin cannot create users.
#[sqlx::test(migrations = "./migrations")]
async fn create_user_non_admin_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "create-noadm", "createnoadm@test.com").await;

    let (status, _) = helpers::post_json(
        &app,
        &user_token,
        "/api/users",
        json!({
            "name": "unauthorized",
            "email": "unauth@test.com",
            "password": "testpass123",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
