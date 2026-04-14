// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Additional integration tests for `src/api/onboarding.rs` coverage gaps.
//!
//! Covers: wizard with provider_key, wizard with cli_token, update settings no-op,
//! verify_oauth_token valid-length, claude auth start non-admin.

mod helpers;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Complete wizard with provider_key
// ---------------------------------------------------------------------------

/// Completing wizard with a provider_key saves it.
#[sqlx::test(migrations = "./migrations")]
async fn complete_wizard_with_provider_key(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/wizard",
        json!({
            "org_type": "solo",
            "provider_key": "sk-ant-api03-test-key-1234567890"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "wizard with key failed: {body}");
    assert_eq!(body["success"], true);

    // Verify the key was saved in user_provider_keys
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_provider_keys WHERE provider = 'anthropic'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(count.0 >= 1, "provider key should be saved");
}

/// Completing wizard with a cli_token saves it.
#[sqlx::test(migrations = "./migrations")]
async fn complete_wizard_with_cli_token(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/wizard",
        json!({
            "org_type": "solo",
            "cli_token": {
                "auth_type": "setup_token",
                "token": "test-oauth-token-value-long-enough-1234567890"
            }
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "wizard with cli_token failed: {body}"
    );
    assert_eq!(body["success"], true);

    // Verify cli credentials were saved
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cli_credentials")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(count.0 >= 1, "cli credentials should be saved");
}

/// Completing wizard with both provider_key and passkey_policy.
#[sqlx::test(migrations = "./migrations")]
async fn complete_wizard_with_key_and_passkey(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/wizard",
        json!({
            "org_type": "startup",
            "provider_key": "sk-ant-api03-combined-test-key",
            "passkey_policy": "optional",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "combined wizard failed: {body}");
    assert_eq!(body["success"], true);
}

// ---------------------------------------------------------------------------
// Update settings edge cases
// ---------------------------------------------------------------------------

/// Update settings with empty body (no org_type) returns current settings.
#[sqlx::test(migrations = "./migrations")]
async fn update_settings_no_changes(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) =
        helpers::patch_json(&app, &admin_token, "/api/onboarding/settings", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    // Should still return valid settings response
    assert!(body["onboarding_completed"].is_boolean());
}

/// Non-admin cannot update settings.
#[sqlx::test(migrations = "./migrations")]
async fn update_settings_non_admin_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "no-upd-set", "noupdset@test.com").await;

    let (status, _) = helpers::patch_json(
        &app,
        &user_token,
        "/api/onboarding/settings",
        json!({ "org_type": "startup" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Update settings with tech_org creates team workspace.
#[sqlx::test(migrations = "./migrations")]
async fn update_settings_tech_org_creates_workspace(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        "/api/onboarding/settings",
        json!({ "org_type": "tech_org" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "tech_org update failed: {body}");

    // Verify workspace was created
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM workspaces WHERE name = 'team'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(count.0 >= 1);
}

// ---------------------------------------------------------------------------
// Claude auth status — non-owner access
// ---------------------------------------------------------------------------

/// Non-admin non-owner accessing claude auth status returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn claude_auth_status_non_owner_non_admin(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "auth-noown", "authnoown@test.com").await;

    // Use a random UUID (session doesn't exist)
    let fake_id = uuid::Uuid::new_v4();
    let (status, _) = helpers::get_json(
        &app,
        &user_token,
        &format!("/api/onboarding/claude-auth/{fake_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Non-admin non-owner cancelling claude auth returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn cancel_claude_auth_non_owner_non_admin(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "auth-nocan", "authnocan@test.com").await;

    let fake_id = uuid::Uuid::new_v4();
    let (status, _) = helpers::delete_json(
        &app,
        &user_token,
        &format!("/api/onboarding/claude-auth/{fake_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Non-admin cannot submit auth code.
#[sqlx::test(migrations = "./migrations")]
async fn submit_auth_code_non_admin_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "code-noadm", "codenoadm@test.com").await;

    let fake_id = uuid::Uuid::new_v4();
    let (status, _) = helpers::post_json(
        &app,
        &user_token,
        &format!("/api/onboarding/claude-auth/{fake_id}/code"),
        json!({ "code": "test-code" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Provider key short suffix
// ---------------------------------------------------------------------------

/// Provider key shorter than 4 characters uses the whole key as suffix.
#[sqlx::test(migrations = "./migrations")]
async fn wizard_provider_key_short_suffix(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/wizard",
        json!({
            "org_type": "solo",
            "provider_key": "abc"  // less than 4 chars
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "short key wizard failed: {body}");
    assert_eq!(body["success"], true);

    // Verify suffix is the full key ("abc")
    let suffix: (String,) = sqlx::query_as(
        "SELECT key_suffix FROM user_provider_keys WHERE provider = 'anthropic' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(suffix.0, "abc");
}
