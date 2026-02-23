//! Integration tests for agent-to-agent spawning (Phase D).

mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

use helpers::{
    admin_login, assign_role, create_project, create_user, get_json, post_json, test_router,
    test_state,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Insert a fake agent session directly in the DB so we don't need a real K8s pod.
async fn insert_session(
    pool: &PgPool,
    project_id: Uuid,
    user_id: Uuid,
    status: &str,
    parent_session_id: Option<Uuid>,
    spawn_depth: i32,
    allowed_child_roles: Option<&[String]>,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO agent_sessions (id, project_id, user_id, prompt, provider, status,
                                    parent_session_id, spawn_depth, allowed_child_roles)
        VALUES ($1, $2, $3, 'test prompt', 'claude-code', $4, $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(project_id)
    .bind(user_id)
    .bind(status)
    .bind(parent_session_id)
    .bind(spawn_depth)
    .bind(allowed_child_roles)
    .execute(pool)
    .await
    .expect("insert session");
    id
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Spawn a child session from a parent session.
#[sqlx::test(migrations = "./migrations")]
async fn spawn_child_session(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);

    let admin_token = admin_login(&app).await;
    let (user_id, token) = create_user(&app, &admin_token, "dev1", "dev1@test.com").await;
    assign_role(&app, &admin_token, user_id, "developer", None, &pool).await;
    let project_id = create_project(&app, &token, "spawn-test", "private").await;

    // Insert a parent session
    let parent_id = insert_session(&pool, project_id, user_id, "running", None, 0, None).await;

    // Spawn child
    let (status, body) = post_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/sessions/{parent_id}/spawn"),
        serde_json::json!({
            "prompt": "Set up CI/CD pipeline"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "spawn failed: {body}");
    assert_eq!(body["status"].as_str(), Some("pending"));
    assert!(body["id"].as_str().is_some());
}

/// Spawn depth limit is enforced.
#[sqlx::test(migrations = "./migrations")]
async fn spawn_depth_limit(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);

    let admin_token = admin_login(&app).await;
    let (user_id, token) = create_user(&app, &admin_token, "dev2", "dev2@test.com").await;
    assign_role(&app, &admin_token, user_id, "developer", None, &pool).await;
    let project_id = create_project(&app, &token, "depth-test", "private").await;

    // Insert a session at max depth (5)
    let session_id = insert_session(&pool, project_id, user_id, "running", None, 5, None).await;

    // Try to spawn — should fail
    let (status, body) = post_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/sessions/{session_id}/spawn"),
        serde_json::json!({ "prompt": "Should fail" }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "depth limit not enforced: {body}"
    );
    let err = body["error"].as_str().unwrap_or("");
    assert!(err.contains("spawn depth"), "unexpected error: {err}");
}

/// List children of a session.
#[sqlx::test(migrations = "./migrations")]
async fn list_children(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);

    let admin_token = admin_login(&app).await;
    let (user_id, token) = create_user(&app, &admin_token, "dev3", "dev3@test.com").await;
    assign_role(&app, &admin_token, user_id, "developer", None, &pool).await;
    let project_id = create_project(&app, &token, "children-test", "private").await;

    // Parent session
    let parent_id = insert_session(&pool, project_id, user_id, "running", None, 0, None).await;

    // Insert children directly
    let child1 = insert_session(
        &pool,
        project_id,
        user_id,
        "pending",
        Some(parent_id),
        1,
        None,
    )
    .await;
    let child2 = insert_session(
        &pool,
        project_id,
        user_id,
        "completed",
        Some(parent_id),
        1,
        None,
    )
    .await;

    let (status, body) = get_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/sessions/{parent_id}/children"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "list children failed: {body}");
    let items = body.as_array().expect("should be array");
    assert_eq!(items.len(), 2);

    let ids: Vec<String> = items
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(ids.contains(&child1.to_string()));
    assert!(ids.contains(&child2.to_string()));
}

/// Spawn requires agent:spawn permission.
#[sqlx::test(migrations = "./migrations")]
async fn spawn_requires_permission(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);

    let admin_token = admin_login(&app).await;
    // Create user with only viewer role (no agent:spawn)
    let (user_id, token) = create_user(&app, &admin_token, "viewer1", "viewer1@test.com").await;
    assign_role(&app, &admin_token, user_id, "viewer", None, &pool).await;
    let project_id = create_project(&app, &admin_token, "perm-test", "private").await;

    // Insert session owned by admin
    let admin_id: (Uuid,) = sqlx::query_as("SELECT id FROM users WHERE name = 'admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let session_id = insert_session(&pool, project_id, admin_id.0, "running", None, 0, None).await;

    // Viewer tries to spawn — should be forbidden
    let (status, _body) = post_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/sessions/{session_id}/spawn"),
        serde_json::json!({ "prompt": "should fail" }),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Parent-child chain: human → parent → child tracks lineage.
#[sqlx::test(migrations = "./migrations")]
async fn spawn_chain_tracks_lineage(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);

    let admin_token = admin_login(&app).await;
    let (user_id, token) = create_user(&app, &admin_token, "dev4", "dev4@test.com").await;
    assign_role(&app, &admin_token, user_id, "developer", None, &pool).await;
    let project_id = create_project(&app, &token, "chain-test", "private").await;

    // Parent at depth 0
    let parent_id = insert_session(&pool, project_id, user_id, "running", None, 0, None).await;

    // Spawn child (depth 1)
    let (status, child_body) = post_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/sessions/{parent_id}/spawn"),
        serde_json::json!({ "prompt": "First child" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let child_id = Uuid::parse_str(child_body["id"].as_str().unwrap()).unwrap();

    // Verify child in DB has correct parent and depth
    let row: (Option<Uuid>, i32) =
        sqlx::query_as("SELECT parent_session_id, spawn_depth FROM agent_sessions WHERE id = $1")
            .bind(child_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(row.0, Some(parent_id));
    assert_eq!(row.1, 1);
}

/// Spawn preserves parent's user_id (original human).
#[sqlx::test(migrations = "./migrations")]
async fn spawn_preserves_original_user(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);

    let admin_token = admin_login(&app).await;
    let (user_id, token) = create_user(&app, &admin_token, "dev5", "dev5@test.com").await;
    assign_role(&app, &admin_token, user_id, "developer", None, &pool).await;
    let project_id = create_project(&app, &token, "user-test", "private").await;

    let parent_id = insert_session(&pool, project_id, user_id, "running", None, 0, None).await;

    let (status, child_body) = post_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/sessions/{parent_id}/spawn"),
        serde_json::json!({ "prompt": "Child task" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Child should have the same user_id as parent (original human)
    let child_user_id = child_body["user_id"].as_str().unwrap();
    assert_eq!(child_user_id, user_id.to_string());
}

/// Empty children list returns empty array.
#[sqlx::test(migrations = "./migrations")]
async fn list_children_empty(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);

    let admin_token = admin_login(&app).await;
    let (user_id, token) = create_user(&app, &admin_token, "dev6", "dev6@test.com").await;
    assign_role(&app, &admin_token, user_id, "developer", None, &pool).await;
    let project_id = create_project(&app, &token, "empty-test", "private").await;

    let session_id = insert_session(&pool, project_id, user_id, "running", None, 0, None).await;

    let (status, body) = get_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/sessions/{session_id}/children"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let items = body.as_array().unwrap();
    assert!(items.is_empty());
}

/// Wrong project ID for session returns 404 on spawn.
#[sqlx::test(migrations = "./migrations")]
async fn spawn_wrong_project_returns_404(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);

    let admin_token = admin_login(&app).await;
    let (user_id, token) = create_user(&app, &admin_token, "dev7", "dev7@test.com").await;
    assign_role(&app, &admin_token, user_id, "developer", None, &pool).await;
    let project_a = create_project(&app, &token, "project-a", "private").await;
    let project_b = create_project(&app, &token, "project-b", "private").await;

    // Session belongs to project_a
    let session_id = insert_session(&pool, project_a, user_id, "running", None, 0, None).await;

    // Try to spawn via project_b
    let (status, _body) = post_json(
        &app,
        &token,
        &format!("/api/projects/{project_b}/sessions/{session_id}/spawn"),
        serde_json::json!({ "prompt": "should fail" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Prompt validation: empty prompt rejected.
#[sqlx::test(migrations = "./migrations")]
async fn spawn_empty_prompt_rejected(pool: PgPool) {
    let state = test_state(pool.clone()).await;
    let app = test_router(state);

    let admin_token = admin_login(&app).await;
    let (user_id, token) = create_user(&app, &admin_token, "dev8", "dev8@test.com").await;
    assign_role(&app, &admin_token, user_id, "developer", None, &pool).await;
    let project_id = create_project(&app, &token, "validation-test", "private").await;

    let session_id = insert_session(&pool, project_id, user_id, "running", None, 0, None).await;

    let (status, _body) = post_json(
        &app,
        &token,
        &format!("/api/projects/{project_id}/sessions/{session_id}/spawn"),
        serde_json::json!({ "prompt": "" }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}
