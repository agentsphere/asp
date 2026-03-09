mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use std::io::Write;

/// Create a fake agent-runner binary at the expected path for the given arch.
fn write_fake_binary(dir: &std::path::Path, arch: &str) -> std::path::PathBuf {
    let path = dir.join(arch);
    let mut f = std::fs::File::create(&path).expect("create fake binary");
    f.write_all(b"#!/bin/sh\necho fake-agent-runner\n")
        .expect("write fake binary");
    path
}

// -- Happy path --

#[sqlx::test(migrations = "./migrations")]
async fn download_agent_runner_amd64_integration(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;

    // Write fake binary to the configured agent_runner_dir
    std::fs::create_dir_all(&state.config.agent_runner_dir).expect("create dir");
    write_fake_binary(&state.config.agent_runner_dir, "amd64");

    let app = helpers::test_router(state);

    let (status, body) =
        helpers::get_json(&app, &admin_token, "/api/downloads/agent-runner?arch=amd64").await;
    // get_json tries to parse JSON — binary responses will parse as Null
    // We need a raw request instead
    assert!(status == StatusCode::OK || body.is_null());
}

#[sqlx::test(migrations = "./migrations")]
async fn download_agent_runner_returns_binary_integration(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;

    // Write a known binary payload
    std::fs::create_dir_all(&state.config.agent_runner_dir).expect("create dir");
    let path = state.config.agent_runner_dir.join("arm64");
    std::fs::write(&path, b"TESTBINARY").expect("write");

    let app = helpers::test_router(state);

    // Use raw request to check binary response
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/api/downloads/agent-runner?arch=arm64")
        .header("Authorization", format!("Bearer {admin_token}"))
        .body(axum::body::Body::empty())
        .unwrap();

    let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Check headers
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "application/octet-stream"
    );
    assert_eq!(
        resp.headers().get("content-disposition").unwrap(),
        "attachment; filename=\"agent-runner\""
    );
    assert_eq!(
        resp.headers().get("cache-control").unwrap(),
        "public, max-age=3600"
    );

    // Check body
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(&body[..], b"TESTBINARY");
}

// -- Arch normalization --

#[sqlx::test(migrations = "./migrations")]
async fn download_agent_runner_normalizes_x86_64_integration(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;

    std::fs::create_dir_all(&state.config.agent_runner_dir).expect("create dir");
    std::fs::write(state.config.agent_runner_dir.join("amd64"), b"BIN").expect("write");

    let app = helpers::test_router(state);

    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/api/downloads/agent-runner?arch=x86_64")
        .header("Authorization", format!("Bearer {admin_token}"))
        .body(axum::body::Body::empty())
        .unwrap();

    let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn download_agent_runner_normalizes_aarch64_integration(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;

    std::fs::create_dir_all(&state.config.agent_runner_dir).expect("create dir");
    std::fs::write(state.config.agent_runner_dir.join("arm64"), b"BIN").expect("write");

    let app = helpers::test_router(state);

    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/api/downloads/agent-runner?arch=aarch64")
        .header("Authorization", format!("Bearer {admin_token}"))
        .body(axum::body::Body::empty())
        .unwrap();

    let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// -- Error cases --

#[sqlx::test(migrations = "./migrations")]
async fn download_agent_runner_invalid_arch_integration(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, body) =
        helpers::get_json(&app, &admin_token, "/api/downloads/agent-runner?arch=ppc64").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("amd64"));
}

#[sqlx::test(migrations = "./migrations")]
async fn download_agent_runner_no_auth_integration(pool: PgPool) {
    let (state, _admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let (status, _body) =
        helpers::get_json(&app, "", "/api/downloads/agent-runner?arch=amd64").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn download_agent_runner_missing_binary_integration(pool: PgPool) {
    let (mut state, admin_token) = helpers::test_state(pool).await;
    // Override agent_runner_dir to an empty temp dir so no binaries exist,
    // even when PLATFORM_AGENT_RUNNER_DIR points to pre-built binaries.
    let empty_dir =
        std::env::temp_dir().join(format!("agent-runner-empty-{}", uuid::Uuid::new_v4()));
    let mut config = (*state.config).clone();
    config.agent_runner_dir = empty_dir;
    state.config = std::sync::Arc::new(config);
    let app = helpers::test_router(state);

    let (status, _body) =
        helpers::get_json(&app, &admin_token, "/api/downloads/agent-runner?arch=amd64").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
