//! Integration tests for the feature flags CRUD API (`src/api/flags.rs`).
//!
//! Covers all 12 handlers: create, list, get, update, delete, toggle,
//! add_rule, delete_rule, set_override, delete_override, flag_history,
//! and evaluate_flags. Also tests permission boundaries.

mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

use helpers::{
    assign_role, create_project, create_user, delete_json, get_json, patch_json, post_json,
    put_json, test_router, test_state,
};

// ---------------------------------------------------------------------------
// 1. create_flag — POST boolean flag, verify response fields
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_flag(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-create", "private").await;

    let (status, body) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({
            "key": "dark_mode",
            "flag_type": "boolean",
            "default_value": true,
            "description": "Enable dark mode"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["key"], "dark_mode");
    assert_eq!(body["flag_type"], "boolean");
    assert_eq!(body["default_value"], true);
    assert_eq!(body["enabled"], false); // new flags default to disabled
    assert_eq!(body["description"], "Enable dark mode");
    assert_eq!(body["project_id"], project_id.to_string());
    assert!(body["id"].as_str().is_some());
    assert!(body["created_at"].as_str().is_some());
    assert!(body["updated_at"].as_str().is_some());
}

// ---------------------------------------------------------------------------
// 2. create_flag_invalid_key — empty/special chars -> 400
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_flag_invalid_key(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-invalid", "private").await;

    // Empty key
    let (status, _) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({
            "key": "",
            "default_value": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Key with special chars (check_name rejects characters outside alphanumeric + -_.)
    let (status, _) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({
            "key": "bad key!@#",
            "default_value": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// 3. create_flag_duplicate — same key twice -> 409
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_flag_duplicate(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-dup", "private").await;

    let payload = serde_json::json!({
        "key": "feature_x",
        "default_value": false,
        "environment": "staging"
    });

    let (status, _) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        payload.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Second create with same key and same environment
    let (status, body) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        payload,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("already exists"),
        "expected conflict message, got: {body}"
    );
}

// ---------------------------------------------------------------------------
// 4. list_flags — create 2, list, verify count
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn list_flags(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-list", "private").await;

    // Create two flags
    for key in &["flag_alpha", "flag_beta"] {
        let (status, _) = post_json(
            &app,
            &admin_token,
            &format!("/api/projects/{project_id}/flags"),
            serde_json::json!({ "key": key, "default_value": false }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let (status, body) = get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);
    let items = body["items"].as_array().expect("items should be array");
    assert_eq!(items.len(), 2);

    // Ordered by key ASC
    assert_eq!(items[0]["key"], "flag_alpha");
    assert_eq!(items[1]["key"], "flag_beta");
}

// ---------------------------------------------------------------------------
// 5. get_flag — create, get by key, verify fields
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn get_flag(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-get", "private").await;

    let (status, created) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({
            "key": "my_flag",
            "flag_type": "json",
            "default_value": {"color": "blue"},
            "description": "A JSON flag"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/my_flag"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], created["id"]);
    assert_eq!(body["key"], "my_flag");
    assert_eq!(body["flag_type"], "json");
    assert_eq!(body["default_value"], serde_json::json!({"color": "blue"}));
    assert_eq!(body["description"], "A JSON flag");
}

// ---------------------------------------------------------------------------
// 6. update_flag — PATCH description, verify
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn update_flag(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-update", "private").await;

    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({
            "key": "updatable",
            "default_value": false,
            "description": "original"
        }),
    )
    .await;

    let (status, body) = patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/updatable"),
        serde_json::json!({
            "description": "updated description",
            "default_value": true
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["description"], "updated description");
    assert_eq!(body["default_value"], true);
    assert_eq!(body["key"], "updatable");
}

// ---------------------------------------------------------------------------
// 7. delete_flag — DELETE, verify gone
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn delete_flag(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-delete", "private").await;

    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "doomed", "default_value": false }),
    )
    .await;

    let (status, _) = delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/doomed"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // GET should now 404
    let (status, _) = get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/doomed"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 8. toggle_flag — create disabled, toggle, verify enabled
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn toggle_flag(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-toggle", "private").await;

    // Create flag (enabled defaults to false)
    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "toggler", "default_value": false }),
    )
    .await;

    // Toggle on
    let (status, body) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/toggler/toggle"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], true);

    // Toggle off
    let (status, body) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/toggler/toggle"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
}

// ---------------------------------------------------------------------------
// 9. add_rule — add percentage rule, verify
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn add_rule(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-rule", "private").await;

    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "rollout", "default_value": false }),
    )
    .await;

    let (status, body) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/rollout/rules"),
        serde_json::json!({
            "rule_type": "percentage",
            "percentage": 50,
            "serve_value": true,
            "priority": 10
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["rule_type"], "percentage");
    assert_eq!(body["percentage"], 50);
    assert_eq!(body["serve_value"], true);
    assert_eq!(body["priority"], 10);
    assert_eq!(body["enabled"], true);
    assert!(body["id"].as_str().is_some());
    assert!(body["flag_id"].as_str().is_some());
}

// ---------------------------------------------------------------------------
// 10. add_rule_invalid — invalid rule_type -> 400
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn add_rule_invalid(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-rule-inv", "private").await;

    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "r_flag", "default_value": false }),
    )
    .await;

    // Invalid rule_type
    let (status, _) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/r_flag/rules"),
        serde_json::json!({
            "rule_type": "nonsense",
            "serve_value": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Percentage rule without percentage field
    let (status, _) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/r_flag/rules"),
        serde_json::json!({
            "rule_type": "percentage",
            "serve_value": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Percentage out of range
    let (status, _) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/r_flag/rules"),
        serde_json::json!({
            "rule_type": "percentage",
            "percentage": 150,
            "serve_value": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// 11. delete_rule — add then delete rule
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn delete_rule(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-delrule", "private").await;

    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "ruleflag", "default_value": false }),
    )
    .await;

    let (status, rule_body) = post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/ruleflag/rules"),
        serde_json::json!({
            "rule_type": "user_id",
            "attribute_values": ["some-user"],
            "serve_value": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let rule_id = rule_body["id"].as_str().unwrap();

    let (status, _) = delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/ruleflag/rules/{rule_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Deleting same rule again -> 404
    let (status, _) = delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/ruleflag/rules/{rule_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 12. set_override — set user override, verify
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn set_override(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-override", "private").await;

    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "beta", "default_value": false }),
    )
    .await;

    // Enable the flag so overrides take effect during evaluation
    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/beta/toggle"),
        serde_json::json!({}),
    )
    .await;

    // Use a real user so the FK constraint on feature_flag_overrides.user_id is satisfied
    let (target_user_id, _target_token) =
        create_user(&app, &admin_token, "flag-target", "flag-target@test.com").await;
    let (status, body) = put_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/beta/overrides/{target_user_id}"),
        serde_json::json!({ "serve_value": true }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");

    // Verify via evaluation that the override is in effect
    let (status, body) = post_json(
        &app,
        &admin_token,
        "/api/flags/evaluate",
        serde_json::json!({
            "project_id": project_id,
            "keys": ["beta"],
            "user_id": target_user_id.to_string()
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["beta"], true);
}

// ---------------------------------------------------------------------------
// 13. delete_override — set then delete
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn delete_override(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-deloverride", "private").await;

    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "gamma", "default_value": false }),
    )
    .await;

    // Use a real user so the FK constraint on feature_flag_overrides.user_id is satisfied
    let (target_user_id, _target_token) = create_user(
        &app,
        &admin_token,
        "flag-deltarget",
        "flag-deltarget@test.com",
    )
    .await;

    // Set override
    put_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/gamma/overrides/{target_user_id}"),
        serde_json::json!({ "serve_value": true }),
    )
    .await;

    // Delete override
    let (status, _) = delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/gamma/overrides/{target_user_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Deleting again -> 404
    let (status, _) = delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/gamma/overrides/{target_user_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 14. flag_history — do several operations, check history entries
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn flag_history(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-history", "private").await;

    // Create flag -> history: "created"
    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "tracked", "default_value": false }),
    )
    .await;

    // Toggle -> history: "toggled"
    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/tracked/toggle"),
        serde_json::json!({}),
    )
    .await;

    // Update -> history: "updated"
    patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/tracked"),
        serde_json::json!({ "description": "changed" }),
    )
    .await;

    // Add rule -> history: "rule_added"
    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/tracked/rules"),
        serde_json::json!({
            "rule_type": "percentage",
            "percentage": 25,
            "serve_value": true
        }),
    )
    .await;

    // Query history
    let (status, body) = get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags/tracked/history"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 4);

    let items = body["items"].as_array().expect("items should be array");
    assert_eq!(items.len(), 4);

    // History is ordered DESC by created_at, so most recent first
    let actions: Vec<&str> = items
        .iter()
        .map(|i| i["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions[0], "rule_added");
    assert_eq!(actions[1], "updated");
    assert_eq!(actions[2], "toggled");
    assert_eq!(actions[3], "created");

    // Each entry should have flag_id, id, and created_at
    for item in items {
        assert!(item["id"].as_str().is_some());
        assert!(item["flag_id"].as_str().is_some());
        assert!(item["created_at"].as_str().is_some());
        assert!(item["actor_id"].as_str().is_some());
    }
}

// ---------------------------------------------------------------------------
// 15. flag_requires_manage_permission — viewer -> 404 for mutations
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn flag_requires_manage_permission(pool: PgPool) {
    let (state, admin_token) = test_state(pool.clone()).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-perm", "private").await;

    // Create a viewer user with only project:read
    let (viewer_id, viewer_token) =
        create_user(&app, &admin_token, "flagviewer", "flagviewer@test.com").await;
    assign_role(
        &app,
        &admin_token,
        viewer_id,
        "viewer",
        Some(project_id),
        &pool,
    )
    .await;

    // Admin creates a flag for the permission tests
    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "secret_flag", "default_value": false }),
    )
    .await;

    // Viewer cannot create flags (require_flag_manage -> 404)
    let (status, _) = post_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({ "key": "new_flag", "default_value": true }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Viewer cannot update flags
    let (status, _) = patch_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags/secret_flag"),
        serde_json::json!({ "description": "hacked" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Viewer cannot delete flags
    let (status, _) = delete_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags/secret_flag"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Viewer cannot toggle flags
    let (status, _) = post_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags/secret_flag/toggle"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Viewer cannot add rules
    let (status, _) = post_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags/secret_flag/rules"),
        serde_json::json!({
            "rule_type": "percentage",
            "percentage": 50,
            "serve_value": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Viewer cannot set overrides
    let target = Uuid::new_v4();
    let (status, _) = put_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags/secret_flag/overrides/{target}"),
        serde_json::json!({ "serve_value": true }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Viewer cannot delete overrides
    let (status, _) = delete_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags/secret_flag/overrides/{target}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 16. flag_allows_read — viewer -> 200 for list/get/history
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn flag_allows_read(pool: PgPool) {
    let (state, admin_token) = test_state(pool.clone()).await;
    let app = test_router(state);
    let project_id = create_project(&app, &admin_token, "flag-read", "private").await;

    // Create viewer
    let (viewer_id, viewer_token) =
        create_user(&app, &admin_token, "flagreader", "flagreader@test.com").await;
    assign_role(
        &app,
        &admin_token,
        viewer_id,
        "viewer",
        Some(project_id),
        &pool,
    )
    .await;

    // Admin creates a flag
    post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/flags"),
        serde_json::json!({
            "key": "readable",
            "default_value": true,
            "description": "visible to viewers"
        }),
    )
    .await;

    // Viewer can list flags
    let (status, body) = get_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);

    // Viewer can get a flag by key
    let (status, body) = get_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags/readable"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["key"], "readable");

    // Viewer can read history
    let (status, body) = get_json(
        &app,
        &viewer_token,
        &format!("/api/projects/{project_id}/flags/readable/history"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1); // "created" entry

    // Viewer can evaluate flags
    let (status, body) = post_json(
        &app,
        &viewer_token,
        "/api/flags/evaluate",
        serde_json::json!({
            "project_id": project_id,
            "keys": ["readable"]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["readable"], true);
}
