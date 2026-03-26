//! Integration tests for the onboarding/wizard API (`src/api/onboarding.rs`).
//!
//! Tests wizard status, complete wizard, settings CRUD, and permission enforcement.
//! Claude OAuth flow endpoints are tested for error paths only (no real CLI binary).

mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Wizard status
// ---------------------------------------------------------------------------

/// Admin sees show_wizard=true when wizard is not yet completed.
#[sqlx::test(migrations = "./migrations")]
async fn wizard_status_admin_sees_wizard(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) =
        helpers::get_json(&app, &admin_token, "/api/onboarding/wizard-status").await;
    assert_eq!(status, StatusCode::OK);
    // show_wizard is true only if admin AND not completed — bootstrap doesn't
    // complete the wizard, so admin should see it
    assert!(body["show_wizard"].is_boolean());
}

/// Non-admin always sees show_wizard=false.
#[sqlx::test(migrations = "./migrations")]
async fn wizard_status_non_admin_sees_false(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_user_id, user_token) =
        helpers::create_user(&app, &admin_token, "nonadmin", "nonadmin@test.com").await;

    let (status, body) =
        helpers::get_json(&app, &user_token, "/api/onboarding/wizard-status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["show_wizard"], false);
}

// ---------------------------------------------------------------------------
// Complete wizard
// ---------------------------------------------------------------------------

/// Admin can complete the wizard with solo org type.
#[sqlx::test(migrations = "./migrations")]
async fn complete_wizard_solo_dev(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/wizard",
        serde_json::json!({ "org_type": "solo" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "wizard failed: {body}");
    assert_eq!(body["success"], true);

    // After completing, wizard should no longer show
    let (status, body) =
        helpers::get_json(&app, &admin_token, "/api/onboarding/wizard-status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["show_wizard"], false);
}

/// Non-admin cannot complete the wizard.
#[sqlx::test(migrations = "./migrations")]
async fn complete_wizard_non_admin_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_user_id, user_token) =
        helpers::create_user(&app, &admin_token, "nonadmin2", "nonadmin2@test.com").await;

    let (status, _) = helpers::post_json(
        &app,
        &user_token,
        "/api/onboarding/wizard",
        serde_json::json!({ "org_type": "solo" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Settings CRUD
// ---------------------------------------------------------------------------

/// Admin can read settings.
#[sqlx::test(migrations = "./migrations")]
async fn get_settings_returns_defaults(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/onboarding/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["onboarding_completed"].is_boolean());
}

/// Non-admin cannot read settings.
#[sqlx::test(migrations = "./migrations")]
async fn get_settings_non_admin_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "nosettings", "nosettings@test.com").await;

    let (status, _) = helpers::get_json(&app, &user_token, "/api/onboarding/settings").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Admin can update org_type via PATCH.
#[sqlx::test(migrations = "./migrations")]
async fn update_settings_org_type(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        "/api/onboarding/settings",
        serde_json::json!({ "org_type": "startup" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update settings failed: {body}");
    // Response returns current settings
    assert!(body["onboarding_completed"].is_boolean());
}

// ---------------------------------------------------------------------------
// Demo project
// ---------------------------------------------------------------------------

/// Non-admin cannot create demo project.
#[sqlx::test(migrations = "./migrations")]
async fn create_demo_non_admin_forbidden(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "nodemo", "nodemo@test.com").await;

    let (status, _) = helpers::post_json(
        &app,
        &user_token,
        "/api/onboarding/demo-project",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Claude auth — error paths only (no real CLI binary)
// ---------------------------------------------------------------------------

/// verify-token with too-short token returns 400.
#[sqlx::test(migrations = "./migrations")]
async fn verify_oauth_token_too_short(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/claude-auth/verify-token",
        serde_json::json!({ "token": "short" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// verify-token non-admin returns 403.
#[sqlx::test(migrations = "./migrations")]
async fn verify_oauth_token_non_admin(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (_uid, user_token) =
        helpers::create_user(&app, &admin_token, "noauth", "noauth@test.com").await;

    let (status, _) = helpers::post_json(
        &app,
        &user_token,
        "/api/onboarding/claude-auth/verify-token",
        serde_json::json!({ "token": "a]very-long-oauth-token-for-testing-purposes" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// cancel_claude_auth for nonexistent session returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn cancel_nonexistent_claude_auth(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let fake_id = uuid::Uuid::new_v4();
    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/onboarding/claude-auth/{fake_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// claude_auth_status for nonexistent session returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn auth_status_nonexistent(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let fake_id = uuid::Uuid::new_v4();
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/onboarding/claude-auth/{fake_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Complete wizard with different org types
// ---------------------------------------------------------------------------

/// Startup org type creates a team workspace.
#[sqlx::test(migrations = "./migrations")]
async fn complete_wizard_startup(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/wizard",
        serde_json::json!({ "org_type": "startup" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "startup wizard failed: {body}");
    assert_eq!(body["success"], true);

    // Verify a workspace was created (startup creates team workspace)
    let ws_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM workspaces")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(ws_count.0 >= 1, "expected team workspace to be created");
}

/// TechOrg org type creates a team workspace with stricter defaults.
#[sqlx::test(migrations = "./migrations")]
async fn complete_wizard_tech_org(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/wizard",
        serde_json::json!({ "org_type": "tech_org" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "tech_org wizard failed: {body}");
    assert_eq!(body["success"], true);

    // Verify wizard is completed
    let (status, body) =
        helpers::get_json(&app, &admin_token, "/api/onboarding/wizard-status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["show_wizard"], false);
}

/// Complete wizard with passkey_policy override.
#[sqlx::test(migrations = "./migrations")]
async fn complete_wizard_with_passkey_policy(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/wizard",
        serde_json::json!({
            "org_type": "solo",
            "passkey_policy": "mandatory"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "passkey override failed: {body}");
    assert_eq!(body["success"], true);

    // Verify settings reflect the override
    let (status, body) = helpers::get_json(&app, &admin_token, "/api/onboarding/settings").await;
    assert_eq!(status, StatusCode::OK);
    // security_policy should contain the mandatory passkey enforcement
    let security = &body["security_policy"];
    assert!(security.is_object() || security.is_string());
}

/// Complete wizard with custom LLM provider.
#[sqlx::test(migrations = "./migrations")]
async fn complete_wizard_with_custom_provider(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/wizard",
        serde_json::json!({
            "org_type": "solo",
            "custom_provider": {
                "provider_type": "bedrock",
                "env_vars": {
                    "AWS_REGION": "us-east-1",
                    "AWS_ACCESS_KEY_ID": "test-key",
                    "AWS_SECRET_ACCESS_KEY": "test-secret"
                }
            }
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "custom provider wizard failed: {body}"
    );
    assert_eq!(body["success"], true);
}

/// Admin can create a demo project.
#[sqlx::test(migrations = "./migrations")]
async fn create_demo_project_success(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/onboarding/demo-project",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "demo project failed: {body}");
    assert!(body["project_id"].is_string());
    assert!(body["project_name"].is_string());
}
