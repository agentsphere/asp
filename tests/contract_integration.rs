//! Contract integration tests — verify JSON response shapes match what the UI expects.
//!
//! These tests don't duplicate business logic tests. They focus on:
//! - Correct field names (catches serde rename bugs)
//! - Correct field types (string, number, bool, null, array, object)
//! - ListResponse wrapper on list endpoints (`{items: [...], total: N}`)
//! - Nullable fields can actually be null
//!
//! Every assertion here corresponds to a field access in the UI code.

mod helpers;

use axum::http::StatusCode;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use helpers::{
    admin_login, create_project, create_user, set_user_api_key, test_router, test_state,
};

// =========================================================================
// Helpers — reusable shape assertions
// =========================================================================

/// Assert a value is a non-empty UUID string.
fn assert_uuid(v: &Value, ctx: &str) {
    let s = v
        .as_str()
        .unwrap_or_else(|| panic!("{ctx}: expected string"));
    Uuid::parse_str(s).unwrap_or_else(|_| panic!("{ctx}: expected UUID, got {s}"));
}

/// Assert a value is an ISO 8601 timestamp string.
fn assert_timestamp(v: &Value, ctx: &str) {
    let s = v
        .as_str()
        .unwrap_or_else(|| panic!("{ctx}: expected string"));
    assert!(
        s.contains('T') || s.contains('-'),
        "{ctx}: expected timestamp, got {s}"
    );
}

/// Assert a value is a JSON number (i64 or f64).
fn assert_number(v: &Value, ctx: &str) {
    assert!(v.is_number(), "{ctx}: expected number, got {v}");
}

/// Assert ListResponse shape: {items: [...], total: N}
fn assert_list_response<'a>(body: &'a Value, ctx: &str) -> &'a Vec<Value> {
    assert!(body["items"].is_array(), "{ctx}: missing items array");
    assert_number(&body["total"], &format!("{ctx}.total"));
    body["items"].as_array().unwrap()
}

// =========================================================================
// P0: Auth endpoints
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_login_response(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);

    let (status, body) = helpers::post_json(
        &app,
        "",
        "/api/auth/login",
        serde_json::json!({"name": "admin", "password": "testpassword"}),
    )
    .await;

    assert_eq!(status, StatusCode::OK);

    // LoginResponse: { token, expires_at, user: { id, name, email, ... } }
    assert!(body["token"].is_string(), "missing token");
    assert_timestamp(&body["expires_at"], "login.expires_at");

    let user = &body["user"];
    assert_uuid(&user["id"], "login.user.id");
    assert!(user["name"].is_string(), "missing user.name");
    assert!(user["email"].is_string(), "missing user.email");
    assert!(user["is_active"].is_boolean(), "missing user.is_active");
    assert_timestamp(&user["created_at"], "login.user.created_at");
    // display_name and user_type are nullable/present
    assert!(
        user["user_type"].is_string(),
        "missing user.user_type: {user}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_me_response(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::get_json(&app, &token, "/api/auth/me").await;

    assert_eq!(status, StatusCode::OK);

    // User: { id, name, email, display_name, user_type, is_active, created_at, updated_at }
    assert_uuid(&body["id"], "me.id");
    assert!(body["name"].is_string(), "missing name");
    assert!(body["email"].is_string(), "missing email");
    assert!(body["is_active"].is_boolean(), "missing is_active");
    assert!(body["user_type"].is_string(), "missing user_type");
    assert_timestamp(&body["created_at"], "me.created_at");
    assert_timestamp(&body["updated_at"], "me.updated_at");
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_logout(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, _) =
        helpers::post_json(&app, &token, "/api/auth/logout", serde_json::json!({})).await;

    // Logout returns 200 with {ok: true} or 204
    assert!(
        status == StatusCode::OK || status == StatusCode::NO_CONTENT,
        "logout returned {status}"
    );
}

// =========================================================================
// P0: Dashboard endpoints
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_dashboard_stats(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::get_json(&app, &token, "/api/dashboard/stats").await;

    assert_eq!(status, StatusCode::OK);

    // DashboardStats: 6 numeric fields
    assert_number(&body["projects"], "stats.projects");
    assert_number(&body["active_sessions"], "stats.active_sessions");
    assert_number(&body["running_builds"], "stats.running_builds");
    assert_number(&body["failed_builds"], "stats.failed_builds");
    assert_number(&body["healthy_deployments"], "stats.healthy_deployments");
    assert_number(&body["degraded_deployments"], "stats.degraded_deployments");
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_audit_log_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    // Create something to generate audit entries
    create_project(&app, &token, "audit-contract", "private").await;

    let (status, body) = helpers::get_json(&app, &token, "/api/audit-log?limit=10").await;

    assert_eq!(status, StatusCode::OK);

    let items = assert_list_response(&body, "audit-log");
    assert!(!items.is_empty(), "expected at least one audit entry");

    // AuditLogEntry shape
    let entry = &items[0];
    assert_uuid(&entry["id"], "audit.id");
    assert_uuid(&entry["actor_id"], "audit.actor_id");
    assert!(entry["actor_name"].is_string(), "missing actor_name");
    assert!(entry["action"].is_string(), "missing action");
    assert!(entry["resource"].is_string(), "missing resource");
    assert_timestamp(&entry["created_at"], "audit.created_at");
    // resource_id and detail can be null
    assert!(
        entry["resource_id"].is_string() || entry["resource_id"].is_null(),
        "resource_id should be string or null"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_onboarding_status(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::get_json(&app, &token, "/api/onboarding/status").await;

    assert_eq!(status, StatusCode::OK);

    // OnboardingStatus: { has_projects, has_provider_key, needs_onboarding }
    assert!(
        body["has_projects"].is_boolean(),
        "missing has_projects: {body}"
    );
    assert!(
        body["has_provider_key"].is_boolean(),
        "missing has_provider_key: {body}"
    );
    assert!(
        body["needs_onboarding"].is_boolean(),
        "missing needs_onboarding: {body}"
    );
}

// =========================================================================
// P0: Notification endpoints
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_notifications_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::get_json(&app, &token, "/api/notifications?limit=10").await;

    assert_eq!(status, StatusCode::OK);
    assert_list_response(&body, "notifications");
    // Even if empty, shape should be correct
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_notifications_unread_count(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::get_json(&app, &token, "/api/notifications/unread-count").await;

    assert_eq!(status, StatusCode::OK);
    assert_number(&body["count"], "unread_count.count");
}

// =========================================================================
// P0: Project CRUD
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_project_create(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::post_json(
        &app,
        &token,
        "/api/projects",
        serde_json::json!({"name": "contract-proj", "visibility": "private", "description": "A test project"}),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);

    // Project response shape
    assert_uuid(&body["id"], "project.id");
    assert_eq!(body["name"], "contract-proj");
    assert!(body["visibility"].is_string(), "missing visibility");
    assert!(body["default_branch"].is_string(), "missing default_branch");
    assert_timestamp(&body["created_at"], "project.created_at");
    assert_timestamp(&body["updated_at"], "project.updated_at");
    assert_uuid(&body["owner_id"], "project.owner_id");
    // description and display_name can be null
    assert!(
        body["description"].is_string() || body["description"].is_null(),
        "description should be string or null"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_project_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    create_project(&app, &token, "list-proj", "private").await;

    let (status, body) = helpers::get_json(&app, &token, "/api/projects?limit=10").await;

    assert_eq!(status, StatusCode::OK);
    let items = assert_list_response(&body, "projects");
    assert!(!items.is_empty(), "expected at least one project");

    // Each item should be a full Project
    let proj = &items[0];
    assert_uuid(&proj["id"], "project[0].id");
    assert!(proj["name"].is_string(), "missing project name");
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_project_get(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let proj_id = create_project(&app, &token, "get-proj", "private").await;

    let (status, body) = helpers::get_json(&app, &token, &format!("/api/projects/{proj_id}")).await;

    assert_eq!(status, StatusCode::OK);
    assert_uuid(&body["id"], "project.id");
    assert_eq!(body["name"], "get-proj");
    assert!(body["visibility"].is_string(), "missing visibility");
    assert!(body["default_branch"].is_string(), "missing default_branch");
    assert_uuid(&body["owner_id"], "project.owner_id");
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_project_update(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let proj_id = create_project(&app, &token, "update-proj", "private").await;

    let (status, body) = helpers::patch_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}"),
        serde_json::json!({"description": "Updated desc"}),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_uuid(&body["id"], "project.id");
    assert_eq!(body["description"], "Updated desc");
}

// =========================================================================
// P0: Issues
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_issue_crud(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "issue-proj", "private").await;

    // Create issue
    let (status, body) = helpers::post_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/issues"),
        serde_json::json!({"title": "Bug report", "body": "Something broken", "labels": ["bug"]}),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_uuid(&body["id"], "issue.id");
    assert_uuid(&body["project_id"], "issue.project_id");
    assert_number(&body["number"], "issue.number");
    assert_eq!(body["title"], "Bug report");
    assert!(body["status"].is_string(), "missing issue status");
    assert_uuid(&body["author_id"], "issue.author_id");
    assert_timestamp(&body["created_at"], "issue.created_at");
    assert_timestamp(&body["updated_at"], "issue.updated_at");
    // labels should be an array
    assert!(body["labels"].is_array(), "labels should be array");

    let issue_num = body["number"].as_i64().unwrap();

    // List issues
    let (status, list_body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/issues?limit=10"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let items = assert_list_response(&list_body, "issues");
    assert!(!items.is_empty());

    // Get single issue
    let (status, get_body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/issues/{issue_num}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(get_body["title"], "Bug report");

    // Create comment
    let (status, comment) = helpers::post_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/issues/{issue_num}/comments"),
        serde_json::json!({"body": "A comment"}),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_uuid(&comment["id"], "comment.id");
    assert!(comment["body"].is_string(), "missing comment body");
    assert_uuid(&comment["author_id"], "comment.author_id");
    assert_timestamp(&comment["created_at"], "comment.created_at");
    assert_timestamp(&comment["updated_at"], "comment.updated_at");

    // List comments
    let (status, comments) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/issues/{issue_num}/comments"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    // Comments endpoint returns ListResponse (items + total)
    let comment_items = assert_list_response(&comments, "comments");
    assert!(!comment_items.is_empty());
}

// =========================================================================
// P0: Merge Requests
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_merge_request_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "mr-proj", "private").await;

    // MR list should return ListResponse even if empty
    let (status, body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/merge-requests?limit=10"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_list_response(&body, "merge-requests");
}

// =========================================================================
// P0: Pipelines
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_pipeline_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "pipe-proj", "private").await;

    let (status, body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/pipelines?limit=10"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_list_response(&body, "pipelines");
}

// =========================================================================
// P0: Deployments
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_deployment_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "dep-proj", "private").await;

    let (status, body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/deployments?limit=10"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_list_response(&body, "deployments");
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_deployment_with_data(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "dep-data-proj", "private").await;

    // Insert a deployment directly
    let dep_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment, image_ref, desired_status, current_status)
         VALUES ($1, $2, 'staging', 'app:v1', 'active', 'healthy')",
    )
    .bind(dep_id)
    .bind(proj_id)
    .execute(&pool)
    .await
    .unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/deployments?limit=10"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let items = assert_list_response(&body, "deployments");
    assert!(!items.is_empty());

    // Deployment shape
    let dep = &items[0];
    assert_uuid(&dep["id"], "deployment.id");
    assert_uuid(&dep["project_id"], "deployment.project_id");
    assert!(dep["environment"].is_string(), "missing environment");
    assert!(dep["image_ref"].is_string(), "missing image_ref");
    assert!(dep["desired_status"].is_string(), "missing desired_status");
    assert!(dep["current_status"].is_string(), "missing current_status");
    assert_timestamp(&dep["created_at"], "deployment.created_at");
}

// =========================================================================
// P0: Webhooks
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_webhook_crud(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "wh-proj", "private").await;

    // Create webhook
    let (status, body) = helpers::post_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/webhooks"),
        serde_json::json!({"url": "https://example.com/hook", "events": ["push", "issue"]}),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_uuid(&body["id"], "webhook.id");
    assert_uuid(&body["project_id"], "webhook.project_id");
    assert_eq!(body["url"], "https://example.com/hook");
    assert!(body["events"].is_array(), "missing events array");
    assert!(body["active"].is_boolean(), "missing active");
    assert_timestamp(&body["created_at"], "webhook.created_at");

    // List webhooks
    let (status, list_body) =
        helpers::get_json(&app, &token, &format!("/api/projects/{proj_id}/webhooks")).await;

    assert_eq!(status, StatusCode::OK);
    let items = assert_list_response(&list_body, "webhooks");
    assert!(!items.is_empty());
}

// =========================================================================
// P0: Secrets
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_secrets_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "sec-proj", "private").await;

    // Create a secret
    let (status, body) = helpers::post_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/secrets"),
        serde_json::json!({"name": "MY_SECRET", "value": "supersecret", "scope": "pipeline"}),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(body["name"].is_string(), "missing secret name");
    assert!(body["scope"].is_string(), "missing scope");
    assert_timestamp(&body["created_at"], "secret.created_at");
    // Value should NOT be returned
    assert!(
        body["value"].is_null() || body.get("value").is_none(),
        "secret value should not be returned"
    );

    // List secrets
    let (status, list_body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/secrets?limit=10"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let items = assert_list_response(&list_body, "secrets");
    assert!(!items.is_empty());
}

// =========================================================================
// P0: Sessions (Create-App)
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_create_app_session(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    // Admin needs an API key to create app sessions
    let admin_id: (Uuid,) = sqlx::query_as("SELECT id FROM users WHERE name = 'admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    set_user_api_key(&pool, admin_id.0).await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/create-app",
        serde_json::json!({"description": "Build a todo app"}),
    )
    .await;

    // create-app returns the session
    assert_eq!(status, StatusCode::CREATED, "create-app failed: {body}");
    assert_uuid(&body["id"], "session.id");
    assert!(body["status"].is_string(), "missing session status");
    assert_timestamp(&body["created_at"], "session.created_at");
}

// =========================================================================
// P0: Project sessions
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_project_sessions_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "sess-proj", "private").await;

    let (status, body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/sessions?limit=10"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_list_response(&body, "sessions");
}

// =========================================================================
// P0: Admin — Users
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_admin_user_create(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::post_json(
        &app,
        &token,
        "/api/users",
        serde_json::json!({"name": "contract-user", "email": "cu@test.com", "password": "securepass123"}),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_uuid(&body["id"], "user.id");
    assert_eq!(body["name"], "contract-user");
    assert_eq!(body["email"], "cu@test.com");
    assert!(body["is_active"].is_boolean(), "missing is_active");
    assert!(body["user_type"].is_string(), "missing user_type");
    assert_timestamp(&body["created_at"], "user.created_at");
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_admin_user_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::get_json(&app, &token, "/api/users/list?limit=10").await;

    assert_eq!(status, StatusCode::OK);
    let items = assert_list_response(&body, "users/list");
    assert!(!items.is_empty());

    let user = &items[0];
    assert_uuid(&user["id"], "user.id");
    assert!(user["name"].is_string(), "missing name");
    assert!(user["email"].is_string(), "missing email");
}

// =========================================================================
// P0: Admin — Roles
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_admin_roles(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::get_json(&app, &token, "/api/admin/roles").await;

    assert_eq!(status, StatusCode::OK);
    // Roles endpoint returns a bare array
    let roles = body.as_array().expect("roles should be array");
    assert!(!roles.is_empty());

    // Role shape
    let role = &roles[0];
    assert_uuid(&role["id"], "role.id");
    assert!(role["name"].is_string(), "missing role name");
    assert!(role["is_system"].is_boolean(), "missing is_system");
}

// =========================================================================
// P0: Admin — Delegations
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_admin_delegation_shape(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (user_id, _) = create_user(&app, &token, "deleg-target", "deleg@test.com").await;

    let (status, body) = helpers::post_json(
        &app,
        &token,
        "/api/admin/delegations",
        serde_json::json!({
            "delegate_id": user_id.to_string(),
            "permission": "project:read",
            "reason": "contract test"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);

    // Delegation shape — UI uses permission_name (not permission)
    assert_uuid(&body["id"], "delegation.id");
    assert_uuid(&body["delegator_id"], "delegation.delegator_id");
    assert_uuid(&body["delegate_id"], "delegation.delegate_id");
    assert_uuid(&body["permission_id"], "delegation.permission_id");
    assert!(
        body["permission_name"].is_string(),
        "missing permission_name: {body}"
    );
    assert_timestamp(&body["created_at"], "delegation.created_at");
    // Optional fields
    assert!(
        body["project_id"].is_string() || body["project_id"].is_null(),
        "project_id should be string or null"
    );
    assert!(
        body["reason"].is_string() || body["reason"].is_null(),
        "reason should be string or null"
    );
}

// =========================================================================
// P0: Admin — Permissions
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_admin_permissions(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    // Get any role to list its permissions
    let (_, roles) = helpers::get_json(&app, &token, "/api/admin/roles").await;
    let admin_role = roles
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "admin")
        .unwrap();
    let role_id = admin_role["id"].as_str().unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/admin/roles/{role_id}/permissions"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let perms = body.as_array().expect("permissions should be array");
    assert!(!perms.is_empty());

    // Permission shape
    let perm = &perms[0];
    assert_uuid(&perm["id"], "permission.id");
    assert!(perm["name"].is_string(), "missing permission name");
}

// =========================================================================
// P0: API Tokens
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_api_tokens(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    // Create token
    let (status, body) = helpers::post_json(
        &app,
        &token,
        "/api/tokens",
        serde_json::json!({"name": "my-token", "scopes": ["project:read"], "expires_in_days": 30}),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "create token failed: {body}");

    // CreateTokenResponse: { id, name, token, scopes, expires_at, created_at }
    assert_uuid(&body["id"], "token.id");
    assert_eq!(body["name"], "my-token");
    assert!(body["token"].is_string(), "missing token value");
    assert!(
        body["token"].as_str().unwrap().starts_with("plat_"),
        "token should start with plat_"
    );
    assert!(body["scopes"].is_array(), "missing scopes");
    assert_timestamp(&body["created_at"], "token.created_at");

    // List tokens
    let (status, list_body) = helpers::get_json(&app, &token, "/api/tokens").await;

    assert_eq!(status, StatusCode::OK);
    let tokens = list_body.as_array().expect("tokens should be array");
    assert!(!tokens.is_empty());

    // Token list shape (no plaintext token in list)
    let t = &tokens[0];
    assert_uuid(&t["id"], "token.id");
    assert!(t["name"].is_string(), "missing token name");
    assert!(t["scopes"].is_array(), "missing scopes");
}

// =========================================================================
// P0: Preview Deployments
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_preview_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "preview-proj", "private").await;

    let (status, body) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{proj_id}/previews?limit=10"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_list_response(&body, "previews");
}

// =========================================================================
// P0: Observe — Alerts
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_alert_rules_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::get_json(&app, &token, "/api/observe/alerts?limit=10").await;

    assert_eq!(status, StatusCode::OK);
    assert_list_response(&body, "alert-rules");
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_alert_rule_create(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) = helpers::post_json(
        &app,
        &token,
        "/api/observe/alerts",
        serde_json::json!({
            "name": "high-error-rate",
            "query": "metric:http_errors",
            "condition": "gt",
            "threshold": 100.0,
            "window_seconds": 300,
            "channels": ["webhook"],
        }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::CREATED,
        "create alert rule failed: {body}"
    );

    // AlertRule shape — check serde renames
    assert_uuid(&body["id"], "alert_rule.id");
    assert_eq!(body["name"], "high-error-rate");
    assert!(body["query"].is_string(), "missing query");
    assert!(body["condition"].is_string(), "missing condition");
    // threshold can be null
    assert!(
        body["threshold"].is_number() || body["threshold"].is_null(),
        "threshold should be number or null"
    );
    // UI accesses window_seconds (serde rename from for_seconds)
    assert_number(&body["window_seconds"], "alert_rule.window_seconds");
    // UI accesses channels (serde rename from notify_channels)
    assert!(body["channels"].is_array(), "missing channels");
    assert!(body["enabled"].is_boolean(), "missing enabled");
    assert_timestamp(&body["created_at"], "alert_rule.created_at");
}

// =========================================================================
// P0: Observe — Logs & Traces (empty queries)
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_observe_logs_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) =
        helpers::get_json(&app, &token, "/api/observe/logs?limit=10&time_range=1h").await;

    assert_eq!(status, StatusCode::OK);
    assert_list_response(&body, "logs");
}

#[sqlx::test(migrations = "./migrations")]
async fn contract_observe_traces_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;

    let (status, body) =
        helpers::get_json(&app, &token, "/api/observe/traces?limit=10&time_range=1h").await;

    assert_eq!(status, StatusCode::OK);
    assert_list_response(&body, "traces");
}

// =========================================================================
// P0: Git Browser
// =========================================================================

#[sqlx::test(migrations = "./migrations")]
async fn contract_branches_list(pool: PgPool) {
    let state = test_state(pool).await;
    let app = test_router(state);
    let token = admin_login(&app).await;
    let proj_id = create_project(&app, &token, "git-proj", "private").await;

    // Branches for a fresh project (may be empty or have default branch)
    let (status, body) =
        helpers::get_json(&app, &token, &format!("/api/projects/{proj_id}/branches")).await;

    // Could be 200 with empty array or 404 if no repo initialized
    assert!(
        status == StatusCode::OK || status == StatusCode::NOT_FOUND,
        "branches returned {status}: {body}"
    );
    if status == StatusCode::OK {
        assert!(body.is_array(), "branches should be array");
    }
}
