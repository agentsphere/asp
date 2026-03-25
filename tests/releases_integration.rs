mod helpers;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Release CRUD
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_release(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-create", "private").await;

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({
            "tag_name": "v1.0.0",
            "name": "First Release",
            "body": "Release notes here",
            "is_draft": false,
            "is_prerelease": false,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(body["id"].is_string());
    assert_eq!(body["tag_name"], "v1.0.0");
    assert_eq!(body["name"], "First Release");
    assert_eq!(body["body"], "Release notes here");
    assert_eq!(body["is_draft"], false);
    assert_eq!(body["is_prerelease"], false);
    assert_eq!(body["project_id"], project_id.to_string());
}

#[sqlx::test(migrations = "./migrations")]
async fn create_release_invalid_tag(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-badtag", "private").await;

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({
            "tag_name": "",
            "name": "Bad Tag",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn list_releases(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-list", "private").await;

    // Create two releases
    let (s1, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({ "tag_name": "v0.1.0", "name": "Alpha" }),
    )
    .await;
    assert_eq!(s1, StatusCode::CREATED);

    let (s2, _) = helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({ "tag_name": "v0.2.0", "name": "Beta" }),
    )
    .await;
    assert_eq!(s2, StatusCode::CREATED);

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_i64().unwrap(), 2);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_release_by_tag(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-get", "private").await;

    helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({
            "tag_name": "v2.0.0",
            "name": "Major Release",
            "body": "Big changes",
            "is_prerelease": true,
        }),
    )
    .await;

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases/v2.0.0"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tag_name"], "v2.0.0");
    assert_eq!(body["name"], "Major Release");
    assert_eq!(body["body"], "Big changes");
    assert_eq!(body["is_prerelease"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_release_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-nf", "private").await;

    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases/v999.0.0"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn update_release(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-upd", "private").await;

    helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({ "tag_name": "v1.0.0", "name": "Initial" }),
    )
    .await;

    let (status, body) = helpers::patch_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases/v1.0.0"),
        serde_json::json!({ "name": "Updated Name", "is_draft": true }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Updated Name");
    assert_eq!(body["is_draft"], true);
    assert_eq!(body["tag_name"], "v1.0.0");
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_release(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-del", "private").await;

    helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({ "tag_name": "v1.0.0", "name": "Doomed" }),
    )
    .await;

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases/v1.0.0"),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);

    // Subsequent GET should return 404
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases/v1.0.0"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Permission tests
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn release_requires_project_write(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-nowrite", "public").await;

    // Create a viewer user (has project:read but not project:write)
    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "relviewer", "relviewer@test.com").await;
    helpers::assign_role(&app, &admin_token, user_id, "viewer", None, &pool).await;

    // Viewer cannot create releases (requires project:write)
    let (status, _) = helpers::post_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({ "tag_name": "v1.0.0", "name": "Blocked" }),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn release_allows_project_read(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-pubread", "public").await;

    // Create a release as admin
    helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({ "tag_name": "v1.0.0", "name": "Public Release" }),
    )
    .await;

    // Create a viewer user
    let (user_id, user_token) =
        helpers::create_user(&app, &admin_token, "relreader", "relreader@test.com").await;
    helpers::assign_role(&app, &admin_token, user_id, "viewer", None, &pool).await;

    // Viewer can list releases on public project
    let (status, body) = helpers::get_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/releases"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_i64().unwrap(), 1);

    // Viewer can get a single release by tag
    let (status, body) = helpers::get_json(
        &app,
        &user_token,
        &format!("/api/projects/{project_id}/releases/v1.0.0"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Public Release");
}

// ---------------------------------------------------------------------------
// Asset upload & download
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn upload_and_download_asset(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let project_id = helpers::create_project(&app, &admin_token, "rel-asset", "private").await;

    // Create a release
    helpers::post_json(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases"),
        serde_json::json!({ "tag_name": "v1.0.0", "name": "Asset Release" }),
    )
    .await;

    // Upload an asset via multipart
    let file_content = b"hello binary world 1234567890";
    let boundary = "----TestBoundary12345";
    let mut multipart_body = Vec::new();
    multipart_body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    multipart_body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"artifact.bin\"\r\n",
    );
    multipart_body.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
    multipart_body.extend_from_slice(b"\r\n");
    multipart_body.extend_from_slice(file_content);
    multipart_body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/projects/{project_id}/releases/v1.0.0/assets"))
        .header("Authorization", format!("Bearer {admin_token}"))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(multipart_body))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let upload_status = resp.status();
    let upload_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let upload_body: serde_json::Value = serde_json::from_slice(&upload_bytes).unwrap();

    assert_eq!(upload_status, StatusCode::CREATED);
    assert_eq!(upload_body["name"], "artifact.bin");
    assert_eq!(
        upload_body["size_bytes"].as_i64().unwrap(),
        file_content.len() as i64
    );
    assert!(upload_body["id"].is_string());

    let asset_id = upload_body["id"].as_str().unwrap();

    // Download the asset
    let (download_status, download_bytes) = helpers::get_bytes(
        &app,
        &admin_token,
        &format!("/api/projects/{project_id}/releases/v1.0.0/assets/{asset_id}/download"),
    )
    .await;

    assert_eq!(download_status, StatusCode::OK);
    assert_eq!(download_bytes, file_content);
}
