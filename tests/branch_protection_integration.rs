mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Branch Protection Integration Tests
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_protection(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-create", "private").await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "develop",
            "require_pr": true,
            "block_force_push": true,
            "required_approvals": 2,
            "dismiss_stale_reviews": true,
            "required_checks": ["ci/build", "ci/test"],
            "require_up_to_date": true,
            "allow_admin_bypass": false,
            "merge_methods": ["merge", "squash"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(body["id"].is_string());
    assert_eq!(body["project_id"], project_id.to_string());
    assert_eq!(body["pattern"], "develop");
    assert_eq!(body["require_pr"], true);
    assert_eq!(body["block_force_push"], true);
    assert_eq!(body["required_approvals"], 2);
    assert_eq!(body["dismiss_stale_reviews"], true);
    assert_eq!(
        body["required_checks"].as_array().unwrap(),
        &["ci/build", "ci/test"]
    );
    assert_eq!(body["require_up_to_date"], true);
    assert_eq!(body["allow_admin_bypass"], false);
    assert_eq!(
        body["merge_methods"].as_array().unwrap(),
        &["merge", "squash"]
    );
    assert!(body["created_at"].is_string());
    assert!(body["updated_at"].is_string());
}

#[sqlx::test(migrations = "./migrations")]
async fn create_protection_invalid_pattern(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-invalid-pat", "private").await;

    // Empty pattern should fail validation (min length 1)
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "",
            "merge_methods": ["merge"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_protection_invalid_merge_methods_empty(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-invalid-mm", "private").await;

    // Empty merge_methods array should fail
    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "main",
            "merge_methods": [],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error_msg = body["error"].as_str().unwrap_or("");
    assert!(
        error_msg.contains("at least one method"),
        "error should mention at least one method, got: {error_msg}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn create_protection_invalid_merge_methods_value(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-invalid-mmv", "private").await;

    // Invalid merge method value
    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "main",
            "merge_methods": ["fast-forward"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error_msg = body["error"].as_str().unwrap_or("");
    assert!(
        error_msg.contains("invalid merge method"),
        "error should mention invalid merge method, got: {error_msg}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn list_protections(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-list", "private").await;

    // Create two additional rules (project already has auto-created "main" rule)
    let (s1, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "develop",
            "merge_methods": ["merge"],
        }),
    )
    .await;
    assert_eq!(s1, StatusCode::CREATED);

    let (s2, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "release/*",
            "merge_methods": ["squash", "rebase"],
        }),
    )
    .await;
    assert_eq!(s2, StatusCode::CREATED);

    // List — includes auto-created "main" + "develop" + "release/*"
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_i64().unwrap(), 3);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    // Ordered by created_at ASC: auto-created "main", then "develop", then "release/*"
    assert_eq!(items[0]["pattern"], "main");
    assert_eq!(items[1]["pattern"], "develop");
    assert_eq!(items[2]["pattern"], "release/*");
}

#[sqlx::test(migrations = "./migrations")]
async fn get_protection(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-get", "private").await;

    let (status, created) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "develop",
            "require_pr": false,
            "block_force_push": false,
            "required_approvals": 1,
            "dismiss_stale_reviews": false,
            "required_checks": ["lint"],
            "require_up_to_date": false,
            "allow_admin_bypass": true,
            "merge_methods": ["rebase"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let rule_id = created["id"].as_str().unwrap();

    // GET by id
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections/{rule_id}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], rule_id);
    assert_eq!(body["project_id"], project_id.to_string());
    assert_eq!(body["pattern"], "develop");
    assert_eq!(body["require_pr"], false);
    assert_eq!(body["block_force_push"], false);
    assert_eq!(body["required_approvals"], 1);
    assert_eq!(body["dismiss_stale_reviews"], false);
    assert_eq!(body["required_checks"].as_array().unwrap(), &["lint"]);
    assert_eq!(body["require_up_to_date"], false);
    assert_eq!(body["allow_admin_bypass"], true);
    assert_eq!(body["merge_methods"].as_array().unwrap(), &["rebase"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn update_protection(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-update", "private").await;

    let (status, created) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "develop",
            "require_pr": true,
            "block_force_push": true,
            "required_approvals": 1,
            "merge_methods": ["merge"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let rule_id = created["id"].as_str().unwrap();

    // Partial update — only change required_approvals and allow_admin_bypass
    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections/{rule_id}"),
        serde_json::json!({
            "required_approvals": 3,
            "allow_admin_bypass": true,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], rule_id);
    assert_eq!(body["pattern"], "develop"); // unchanged
    assert_eq!(body["require_pr"], true); // unchanged
    assert_eq!(body["block_force_push"], true); // unchanged
    assert_eq!(body["required_approvals"], 3); // updated
    assert_eq!(body["allow_admin_bypass"], true); // updated
    assert_eq!(body["merge_methods"].as_array().unwrap(), &["merge"]); // unchanged
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_protection(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-delete", "private").await;

    let (status, created) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "develop",
            "merge_methods": ["merge"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let rule_id = created["id"].as_str().unwrap();

    // Delete
    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections/{rule_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // GET should now return 404
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections/{rule_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn protection_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-notfound", "private").await;
    let fake_rule_id = Uuid::new_v4();

    // GET nonexistent
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections/{fake_rule_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // PATCH nonexistent
    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections/{fake_rule_id}"),
        serde_json::json!({ "required_approvals": 5 }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // DELETE nonexistent
    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/branch-protections/{fake_rule_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn protection_requires_project_write(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "bp-authz", "public").await;

    // Create a user with viewer role (project:read only, no project:write)
    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "bp-viewer", "bpviewer@test.com").await;
    helpers::assign_role(&app, &admin_token, user_id, "viewer", None, &pool).await;

    // LIST should be forbidden (require_project_write denies viewer)
    let (status, _) = helpers::get_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/branch-protections"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // CREATE should be forbidden
    let (status, _) = helpers::post_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/branch-protections"),
        serde_json::json!({
            "pattern": "main",
            "merge_methods": ["merge"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // GET / PATCH / DELETE with a fake rule_id should also be forbidden
    // (auth check runs before the DB lookup)
    let fake_rule_id = Uuid::new_v4();

    let (status, _) = helpers::get_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/branch-protections/{fake_rule_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = helpers::patch_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/branch-protections/{fake_rule_id}"),
        serde_json::json!({ "required_approvals": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = helpers::delete_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/branch-protections/{fake_rule_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
