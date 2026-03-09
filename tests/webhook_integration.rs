mod helpers;

use std::time::Duration;

use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers;
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// E7: Webhook Integration Tests (22 tests)
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-proj", "public").await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["push", "issue"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(body["id"].is_string());
    assert_eq!(body["url"], "https://example.com/hook");
    let events = body["events"].as_array().unwrap();
    assert!(events.iter().any(|e| e == "push"));
    assert!(events.iter().any(|e| e == "issue"));
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_invalid_url(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-invalid", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "ftp://example.com/hook",
            "events": ["push"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_ssrf_localhost(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-ssrf-lo", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "http://localhost/hook",
            "events": ["push"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_ssrf_private_10(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-ssrf-10", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "http://10.0.0.1/hook",
            "events": ["push"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_ssrf_private_172(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-ssrf-172", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "http://172.16.0.1/hook",
            "events": ["push"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_ssrf_private_192(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-ssrf-192", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "http://192.168.1.1/hook",
            "events": ["push"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_ssrf_metadata(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-ssrf-meta", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "http://169.254.169.254/",
            "events": ["push"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_ssrf_ipv6_loopback(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-ssrf-ipv6", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "http://[::1]/hook",
            "events": ["push"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn list_webhooks(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-list", "public").await;

    for i in 1..=2 {
        helpers::post_json(
            &app,
            &admin_token,
            &format!("/api/projects/{project_id}/webhooks"),
            serde_json::json!({
                "url": format!("https://example.com/hook{i}"),
                "events": ["push"],
            }),
        )
        .await;
    }

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be array");
    assert_eq!(items.len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn update_and_delete_webhook(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-upd-del", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/original",
            "events": ["push"],
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();

    // Update URL
    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
        serde_json::json!({ "url": "https://example.com/updated" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["url"], "https://example.com/updated");

    // Delete
    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Verify gone
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_requires_project_write(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-no-write", "public").await;

    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "whviewer", "whviewer@test.com").await;
    helpers::assign_role(&app, &admin_token, user_id, "viewer", None, &pool).await;

    // Viewer cannot create webhooks (requires project:write)
    let (status, _) = helpers::post_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["push"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_secret_not_exposed(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-secret", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/secret-hook",
            "events": ["push"],
            "secret": "my-super-secret",
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();

    // GET the webhook back — secret should not be in the response
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The WebhookResponse struct does not include secret field
    assert!(body.get("secret").is_none() || body["secret"].is_null());
}

// ---------------------------------------------------------------------------
// Additional webhook coverage tests
// ---------------------------------------------------------------------------

/// Helper: insert webhook directly into DB (bypasses SSRF validation).
async fn insert_webhook(pool: &PgPool, project_id: Uuid, url: &str, events: &[&str]) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO webhooks (id, project_id, url, events, active) VALUES ($1,$2,$3,$4,true)",
    )
    .bind(id)
    .bind(project_id)
    .bind(url)
    .bind(events)
    .execute(pool)
    .await
    .expect("insert webhook");
    id
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_invalid_event(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-bad-event", "public").await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["push", "nonexistent_event"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let msg = body["error"].as_str().unwrap_or("");
    assert!(
        msg.contains("invalid event"),
        "error should mention invalid event, got: {msg}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_empty_events(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-empty-ev", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": [],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_single_webhook(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-get-one", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/single",
            "events": ["push", "mr"],
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], wh_id);
    assert_eq!(body["url"], "https://example.com/single");
    assert_eq!(body["active"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_webhook_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-get-404", "public").await;
    let fake_id = Uuid::new_v4();

    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{fake_id}"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_webhook_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-del-404", "public").await;
    let fake_id = Uuid::new_v4();

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{fake_id}"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn update_webhook_events(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-upd-events", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["push"],
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();

    // Update events from [push] to [mr, build, deploy]
    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
        serde_json::json!({ "events": ["mr", "build", "deploy"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 3);
    assert!(events.iter().any(|e| e == "mr"));
    assert!(events.iter().any(|e| e == "build"));
    assert!(events.iter().any(|e| e == "deploy"));
}

#[sqlx::test(migrations = "./migrations")]
async fn update_webhook_invalid_event(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-upd-bad", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["push"],
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();

    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
        serde_json::json!({ "events": ["bogus_event"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn update_webhook_deactivate(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-deactivate", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["push"],
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();
    assert_eq!(create_body["active"], true);

    // Deactivate
    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
        serde_json::json!({ "active": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], false);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_webhook_endpoint(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-test-ep", "public").await;

    // Insert webhook directly to bypass SSRF checks (test endpoint will try to deliver)
    let wh_id = insert_webhook(
        &pool,
        project_id,
        "https://example.com/test-delivery",
        &["push"],
    )
    .await;

    // Test webhook endpoint should return OK (delivery happens async, may fail, but endpoint itself succeeds)
    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "test webhook failed: {body}");
    assert_eq!(body["ok"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_webhook_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-test-404", "public").await;
    let fake_id = Uuid::new_v4();

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{fake_id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Additional coverage: edge-case paths
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_ssrf_file_scheme(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-ssrf-file", "public").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "file:///etc/passwd",
            "events": ["push"],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_too_many_events(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-too-many", "public").await;

    // 21 events exceed the max of 20
    let events: Vec<String> = (0..21).map(|_| "push".to_string()).collect();

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": events,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let msg = body["error"].as_str().unwrap_or("");
    assert!(
        msg.contains("max 20"),
        "expected max events error, got: {msg}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn update_webhook_reactivate(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-reactivate", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["push"],
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();

    // Deactivate
    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
        serde_json::json!({ "active": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], false);

    // Reactivate
    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
        serde_json::json!({ "active": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn update_webhook_url_only(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-url-only", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/original",
            "events": ["push", "mr"],
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();

    // Update URL only (events should remain unchanged)
    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
        serde_json::json!({ "url": "https://example.com/new-url" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["url"], "https://example.com/new-url");
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 2, "events should be preserved");
}

#[sqlx::test(migrations = "./migrations")]
async fn update_webhook_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-upd-404", "public").await;
    let fake_id = Uuid::new_v4();

    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{fake_id}"),
        serde_json::json!({ "url": "https://example.com/nope" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn update_webhook_too_many_events(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-upd-toomany", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["push"],
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();

    let events: Vec<String> = (0..21).map(|_| "push".to_string()).collect();
    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
        serde_json::json!({ "events": events }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_webhook_with_secret(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-with-secret", "public").await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/signed-hook",
            "events": ["push", "deploy"],
            "secret": "my-webhook-secret",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    // Secret should NOT be returned in response
    assert!(body.get("secret").is_none() || body["secret"].is_null());
    assert_eq!(body["url"], "https://example.com/signed-hook");
}

#[sqlx::test(migrations = "./migrations")]
async fn update_webhook_ssrf_url_rejected(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "wh-upd-ssrf", "public").await;

    let (_, create_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks"),
        serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["push"],
        }),
    )
    .await;
    let wh_id = create_body["id"].as_str().unwrap();

    // Try to update URL to a private IP → should be blocked
    let (status, _) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/webhooks/{wh_id}"),
        serde_json::json!({ "url": "http://10.0.0.1/evil" }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Webhook dispatch integration tests (moved from e2e_webhook.rs)
//
// These test single-endpoint webhook dispatch behavior using wiremock.
// Webhooks are inserted directly into the DB to bypass SSRF validation
// (wiremock binds to 127.0.0.1).
// ---------------------------------------------------------------------------

/// Creating an issue fires the webhook.
#[sqlx::test(migrations = "./migrations")]
async fn webhook_fires_on_issue_create(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let mock_server = MockServer::start().await;

    Mock::given(matchers::method("POST"))
        .and(matchers::path("/webhook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let project_id = helpers::create_project(&app, &admin_token, "wh-issue-fire", "private").await;

    insert_webhook(
        &state.pool,
        project_id,
        &format!("{}/webhook", mock_server.uri()),
        &["issue"],
    )
    .await;

    let (issue_status, issue_body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/issues"),
        serde_json::json!({
            "title": "Test issue for webhook",
        }),
    )
    .await;
    assert_eq!(
        issue_status,
        StatusCode::CREATED,
        "issue create failed: {issue_body}"
    );

    // Wait for async webhook delivery
    tokio::time::sleep(Duration::from_secs(3)).await;

    mock_server.verify().await;
}

/// Webhook with secret sends HMAC-SHA256 signature header.
#[sqlx::test(migrations = "./migrations")]
async fn webhook_hmac_signature(pool: PgPool) {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let mock_server = MockServer::start().await;

    Mock::given(matchers::method("POST"))
        .and(matchers::header_exists("X-Platform-Signature"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let project_id = helpers::create_project(&app, &admin_token, "wh-hmac", "private").await;

    // Insert webhook with secret directly in DB
    let wh_url = format!("{}/webhook", mock_server.uri());
    let wh_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO webhooks (id, project_id, url, events, secret, active) VALUES ($1,$2,$3,$4,$5,true)",
    )
    .bind(wh_id)
    .bind(project_id)
    .bind(&wh_url)
    .bind(&["issue"] as &[&str])
    .bind(Some("test-secret-key"))
    .execute(&state.pool)
    .await
    .expect("insert webhook with secret");

    helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/issues"),
        serde_json::json!({ "title": "HMAC test issue" }),
    )
    .await;

    tokio::time::sleep(Duration::from_secs(3)).await;
    mock_server.verify().await;

    let requests = mock_server.received_requests().await.unwrap();
    assert!(
        !requests.is_empty(),
        "should have received at least one request"
    );
    let req = &requests[0];

    let signature = req
        .headers
        .get("X-Platform-Signature")
        .expect("should have X-Platform-Signature header")
        .to_str()
        .unwrap();
    assert!(
        signature.starts_with("sha256="),
        "signature should start with sha256=, got: {signature}"
    );

    // Verify the HMAC by computing it ourselves
    let mut mac = Hmac::<Sha256>::new_from_slice(b"test-secret-key").expect("HMAC key");
    mac.update(&req.body);
    let expected = hex::encode(mac.finalize().into_bytes());
    assert_eq!(
        signature,
        format!("sha256={expected}"),
        "HMAC signature should match"
    );
}

/// Webhook without secret does not send signature header.
#[sqlx::test(migrations = "./migrations")]
async fn webhook_no_signature_without_secret(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let mock_server = MockServer::start().await;

    Mock::given(matchers::method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let project_id = helpers::create_project(&app, &admin_token, "wh-nosig", "private").await;

    insert_webhook(
        &state.pool,
        project_id,
        &format!("{}/webhook", mock_server.uri()),
        &["issue"],
    )
    .await;

    helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/issues"),
        serde_json::json!({ "title": "No-sig test" }),
    )
    .await;

    tokio::time::sleep(Duration::from_secs(3)).await;
    mock_server.verify().await;

    let requests = mock_server.received_requests().await.unwrap();
    assert!(!requests.is_empty(), "should receive the webhook");
    let req = &requests[0];
    assert!(
        req.headers.get("X-Platform-Signature").is_none(),
        "should NOT have X-Platform-Signature header when no secret"
    );
}

/// Slow webhook target times out without blocking the platform.
#[sqlx::test(migrations = "./migrations")]
async fn webhook_timeout_doesnt_block(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let mock_server = MockServer::start().await;

    // Server takes 15s to respond (longer than the 10s webhook timeout)
    Mock::given(matchers::method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(15)))
        .mount(&mock_server)
        .await;

    let project_id = helpers::create_project(&app, &admin_token, "wh-timeout", "private").await;

    insert_webhook(
        &state.pool,
        project_id,
        &format!("{}/webhook", mock_server.uri()),
        &["issue"],
    )
    .await;

    let start = std::time::Instant::now();
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/issues"),
        serde_json::json!({ "title": "Timeout test" }),
    )
    .await;
    let elapsed = start.elapsed();
    assert_eq!(status, StatusCode::CREATED);

    // The issue creation should return quickly (webhook is async)
    assert!(
        elapsed.as_secs() < 5,
        "issue creation should not block on slow webhook, took {elapsed:?}"
    );
}

/// Webhook concurrency limit — excess deliveries are dropped.
#[sqlx::test(migrations = "./migrations")]
async fn webhook_concurrent_limit(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let mock_server = MockServer::start().await;

    // Slow server to keep connections open (simulating concurrency pressure)
    Mock::given(matchers::method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
        .mount(&mock_server)
        .await;

    let project_id = helpers::create_project(&app, &admin_token, "wh-concurrent", "private").await;

    insert_webhook(
        &state.pool,
        project_id,
        &format!("{}/webhook", mock_server.uri()),
        &["issue"],
    )
    .await;

    // Create many issues rapidly to overwhelm the semaphore
    for i in 0..60 {
        let _ = helpers::post_json(
            &app,
            &admin_token,
            &format!("/api/projects/{project_id}/issues"),
            serde_json::json!({ "title": format!("Concurrent issue {i}") }),
        )
        .await;
    }

    // Wait for deliveries to complete
    tokio::time::sleep(Duration::from_secs(8)).await;

    let requests = mock_server.received_requests().await.unwrap();
    let received = requests.len();

    // We should receive at most 50 (semaphore limit)
    assert!(
        received <= 50,
        "should receive at most 50 concurrent webhooks, got {received}"
    );
    // We should receive at least some (not all dropped)
    assert!(received > 0, "should receive at least some webhooks, got 0");
}
