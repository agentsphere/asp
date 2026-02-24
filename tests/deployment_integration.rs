//! Integration tests for the deployment API (deployments, previews, ops repos).

mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

use helpers::{admin_login, assign_role, create_project, create_user, test_router, test_state};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn setup_deployment(pool: &PgPool, project_id: Uuid, env: &str, image_ref: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO deployments
           (id, project_id, environment, image_ref, desired_status, current_status)
           VALUES ($1, $2, $3, $4, 'active', 'pending')"#,
    )
    .bind(id)
    .bind(project_id)
    .bind(env)
    .bind(image_ref)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn setup_preview(
    pool: &PgPool,
    project_id: Uuid,
    branch_slug: &str,
    image_ref: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    let branch = format!("feature/{branch_slug}");
    sqlx::query(
        r#"INSERT INTO preview_deployments
           (id, project_id, branch, branch_slug, image_ref, desired_status, current_status,
            expires_at)
           VALUES ($1, $2, $3, $4, $5, 'active', 'pending',
                   now() + interval '1 hour')"#,
    )
    .bind(id)
    .bind(project_id)
    .bind(&branch)
    .bind(branch_slug)
    .bind(image_ref)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn setup_history(pool: &PgPool, deployment_id: Uuid, image_ref: &str, action: &str) {
    sqlx::query(
        r#"INSERT INTO deployment_history
           (deployment_id, image_ref, action, status)
           VALUES ($1, $2, $3, 'success')"#,
    )
    .bind(deployment_id)
    .bind(image_ref)
    .bind(action)
    .execute(pool)
    .await
    .unwrap();
}

// ---------------------------------------------------------------------------
// Deployment API tests
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn list_deployments(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-list", "private").await;
    setup_deployment(&pool, project_id, "staging", "app:v1").await;
    setup_deployment(&pool, project_id, "production", "app:v2").await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/deployments"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_deployment_by_env(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-get", "private").await;
    setup_deployment(&pool, project_id, "staging", "myapp:v3").await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/deployments/staging"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "get deployment failed: {body}");
    assert_eq!(body["environment"], "staging");
    assert_eq!(body["image_ref"], "myapp:v3");
}

#[sqlx::test(migrations = "./migrations")]
async fn get_deployment_not_found(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-nf", "private").await;

    // Use a valid env name that doesn't exist
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/deployments/staging"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn update_deployment_image(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-upd", "private").await;
    setup_deployment(&pool, project_id, "staging", "app:old").await;

    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/deployments/staging"),
        serde_json::json!({ "image_ref": "app:new" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update failed: {body}");
    assert_eq!(body["image_ref"], "app:new");
}

#[sqlx::test(migrations = "./migrations")]
async fn update_deployment_desired_status(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-stop", "private").await;
    setup_deployment(&pool, project_id, "staging", "app:v1").await;

    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/deployments/staging"),
        serde_json::json!({ "desired_status": "stopped" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update status failed: {body}");
    assert_eq!(body["desired_status"], "stopped");
}

#[sqlx::test(migrations = "./migrations")]
async fn update_deployment_validation(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-val", "private").await;
    setup_deployment(&pool, project_id, "staging", "app:v1").await;

    // Empty image_ref should be rejected
    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/deployments/staging"),
        serde_json::json!({ "image_ref": "" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn rollback_deployment(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-rb", "private").await;
    setup_deployment(&pool, project_id, "staging", "app:v2").await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/deployments/staging/rollback"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "rollback failed: {body}");
    assert_eq!(body["ok"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn list_deployment_history(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-hist", "private").await;
    let deploy_id = setup_deployment(&pool, project_id, "staging", "app:v1").await;
    setup_history(&pool, deploy_id, "app:v1", "deploy").await;
    setup_history(&pool, deploy_id, "app:v2", "deploy").await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/deployments/staging/history"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "history failed: {body}");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
}

// ---------------------------------------------------------------------------
// Permission tests
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn deployment_read_requires_permission(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-perm-r", "private").await;
    setup_deployment(&pool, project_id, "staging", "app:v1").await;

    // User with no roles
    let (_uid, token) = create_user(&app, &admin_token, "no-deploy", "nodeploy@test.com").await;

    let (status, _) = helpers::get_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/deployments"),
    )
    .await;
    // Private project with no access returns 404 (not 403, to avoid leaking existence)
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "expected 403 or 404, got {status}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn deployment_update_requires_deploy_promote(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-perm-w", "private").await;
    setup_deployment(&pool, project_id, "staging", "app:v1").await;

    let (uid, token) = create_user(&app, &admin_token, "viewer-dep", "viewer@test.com").await;
    assign_role(&app, &admin_token, uid, "viewer", Some(project_id), &pool).await;

    let (status, _) = helpers::patch_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/deployments/staging"),
        serde_json::json!({ "image_ref": "app:hacked" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn deployment_rollback_requires_deploy_promote(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "deploy-perm-rb", "private").await;
    setup_deployment(&pool, project_id, "staging", "app:v1").await;

    let (uid, token) = create_user(&app, &admin_token, "viewer-rb", "viewerrb@test.com").await;
    assign_role(&app, &admin_token, uid, "viewer", Some(project_id), &pool).await;

    let (status, _) = helpers::post_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/deployments/staging/rollback"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Preview API tests
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn list_previews(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "preview-list", "private").await;
    setup_preview(&pool, project_id, "feat-login", "app:feat-login").await;
    setup_preview(&pool, project_id, "feat-signup", "app:feat-signup").await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/previews"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "list previews failed: {body}");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_preview_by_slug(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "preview-get", "private").await;
    setup_preview(&pool, project_id, "feat-x", "app:feat-x").await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/previews/feat-x"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "get preview failed: {body}");
    assert_eq!(body["branch_slug"], "feat-x");
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_preview(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let project_id = create_project(&app, &admin_token, "preview-del", "private").await;
    setup_preview(&pool, project_id, "feat-del", "app:feat-del").await;

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/previews/feat-del"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // After delete (desired_status=stopped), GET filters by desired_status=active → 404
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/previews/feat-del"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Ops repo admin tests
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_ops_repo(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/admin/ops-repos",
        serde_json::json!({
            "name": "deploy-manifests",
            "repo_url": "https://github.com/example/manifests.git",
            "branch": "main",
            "path": "/k8s",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "create ops repo failed: {body}"
    );
    assert_eq!(body["name"], "deploy-manifests");
}

#[sqlx::test(migrations = "./migrations")]
async fn create_ops_repo_ssrf_blocked(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/admin/ops-repos",
        serde_json::json!({
            "name": "ssrf-repo",
            "repo_url": "http://169.254.169.254/metadata",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn list_and_get_ops_repo(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);
    let admin_token = admin_login(&app).await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/admin/ops-repos",
        serde_json::json!({
            "name": "list-repo",
            "repo_url": "https://github.com/example/ops.git",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let repo_id = body["id"].as_str().unwrap();

    // List — returns a plain array, not {"items": [...]}
    let (status, body) = helpers::get_json(&app, &admin_token, "/api/admin/ops-repos").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_array().unwrap().len() >= 1);

    // Get by ID
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/admin/ops-repos/{repo_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "list-repo");
}
