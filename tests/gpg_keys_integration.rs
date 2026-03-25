mod helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

// Real ED25519 GPG key (UID: Admin <admin@localhost>)
const TEST_ED25519_GPG_KEY: &str = r"-----BEGIN PGP PUBLIC KEY BLOCK-----

mDMEaaB6RRYJKwYBBAHaRw8BAQdAmi3YA/Lq9CTPOMJea51Eu+yZWbDFfhh1rUfE
LAHT8q20F0FkbWluIDxhZG1pbkBsb2NhbGhvc3Q+iJMEExYKADsWIQRQVMGx5eUI
A0cYisgMFfJwj0kygAUCaaB6RQIbAwULCQgHAgIiAgYVCgkICwIEFgIDAQIeBwIX
gAAKCRAMFfJwj0kygH4PAQCLFqqAOULMgdTh8ya9+reLefMHooFFqjaIGyeHxYul
kAEA04pCRRL33dMeTgTYpBGMrMdXegCRLPoUq5eKjET5swc=
=JNMc
-----END PGP PUBLIC KEY BLOCK-----";

// RSA GPG key (UID: Admin RSA <admin@localhost>)
const TEST_RSA_GPG_KEY: &str = r"-----BEGIN PGP PUBLIC KEY BLOCK-----

mQENBGmgekgBCACwzmHejX3A8k73fN54fFa9XDcElZan0BwwXhBk92bHusf3HoOp
tsO0MHmC4bRyDPBSFDwbY9frdUQJC9fS2EWW1YGQaI7bYnbn8OJ214s/V4YtcMwQ
VDNiZJVWRuXaUyRcNsXPOlMBz7sktduvfUF6ua8k4BVU1oiIPD6/WqMAN1HIu9Iz
A/PnaDTF98Qe0KlzT7CPsNWR3Z0KBZ3BfiRpv2TVp8cjvvKEoaX6monZYah1cLKR
0/4EWIOcDG2ZcyjGU12KXRQVW0wt6WB4imrqwQH04K2O8ojAnTEdxFAKSdWgQkBU
xBUKnSRAFY3AQtCFaR7UrG1Xba5PlRMy/jKLABEBAAG0G0FkbWluIFJTQSA8YWRt
aW5AbG9jYWxob3N0PokBUQQTAQgAOxYhBPIK3oK1y4AAbvdlLhrBJ6REow9pBQJp
oHpIAhsDBQsJCAcCAiICBhUKCQgLAgQWAgMBAh4HAheAAAoJEBrBJ6REow9pwxcH
/0nSisESGdXLEBLz3MyBFD+2aK43Sehk1pHjTx7X5AHJBV3GdZvFXji3fzsKDZvf
5f1hKghekNYg7OSMQQWBtEGVe+DgyHvOSJa4Hs/UslkS3NQKTisJuEd/VP72v+HM
FB1oBFlgRAr5kUV2cQBQBbWb54iRwy15KkEzdOIhBaex5uFq/pdUNaxRZ8zViJZF
20NjZNEewe1mNQGTPINyqCB5KHGnhnj1OcnDgfURNmVIK5x5H53A2IiJojZWK1yd
pYaiZSmbery+jVVTGpY4nwQa7bd4lS3rMr+X7UogNV8LVq17kAJ0EGS2GBrJJmZT
gnq5d4Jhjk4E5xyaAiuyxcY=
=CqZH
-----END PGP PUBLIC KEY BLOCK-----";

// GPG key with different email (UID: Other User <other@example.com>)
const TEST_MISMATCH_GPG_KEY: &str = r"-----BEGIN PGP PUBLIC KEY BLOCK-----

mDMEaaB1BRYJKwYBBAHaRw8BAQdARLy3pnfpC9xZzFm0p3C3yowaUJwkgae2DgGI
WZivJWu0Hk90aGVyIFVzZXIgPG90aGVyQGV4YW1wbGUuY29tPoiTBBMWCgA7FiEE
FxWKwBZlJmIu0Nq+boJqgnwdY00FAmmgdQUCGwMFCwkIBwICIgIGFQoJCAsCBBYC
AwECHgcCF4AACgkQboJqgnwdY00PygD+OyssgX52vWyzUQmZUXOKrGW8RT0OXfQB
LR+IPE/XK6cA/j6YvUkcTSPKKxlR8cf8PQKdl8Y/k9BqLZmX8rsNI7cG
=ivG3
-----END PGP PUBLIC KEY BLOCK-----";

// ---------------------------------------------------------------------------
// POST /api/users/me/gpg-keys
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_add_gpg_key_success(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({
            "public_key": TEST_ED25519_GPG_KEY,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "add GPG key: {body}");
    assert!(body["id"].is_string());
    assert!(body["fingerprint"].is_string());
    assert!(body["key_id"].is_string());
    assert_eq!(body["can_sign"], true);
    assert!(
        body["emails"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str() == Some("admin@localhost"))
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_add_gpg_key_rsa_success(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({
            "public_key": TEST_RSA_GPG_KEY,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "add RSA GPG key: {body}");
    assert!(body["fingerprint"].as_str().unwrap().len() >= 40);
}

// ---------------------------------------------------------------------------
// GET /api/users/me/gpg-keys
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_list_gpg_keys_empty(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/users/me/gpg-keys").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_list_gpg_keys_with_data(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    // Add a key first
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // List
    let (status, body) = helpers::get_json(&app, &admin_token, "/api/users/me/gpg-keys").await;
    assert_eq!(status, StatusCode::OK);
    let keys = body["items"].as_array().unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys[0]["fingerprint"].is_string());
}

// ---------------------------------------------------------------------------
// GET /api/users/me/gpg-keys/{id}
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_get_gpg_key_by_id(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, add_body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let key_id = add_body["id"].as_str().unwrap();

    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/users/me/gpg-keys/{key_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["public_key_armor"]
            .as_str()
            .unwrap()
            .contains("PGP PUBLIC KEY BLOCK")
    );
    assert_eq!(body["fingerprint"], add_body["fingerprint"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_get_gpg_key_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/users/me/gpg-keys/{}", Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_get_gpg_key_other_users_key_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    // Admin adds a key
    let (status, add_body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let key_id = add_body["id"].as_str().unwrap();

    // Create another user (email matches the mismatch key)
    let (_, other_token) =
        helpers::create_user(&app, &admin_token, "other-user", "other@example.com").await;

    // Other user tries to get admin's key
    let (status, _) = helpers::get_json(
        &app,
        &other_token,
        &format!("/api/users/me/gpg-keys/{key_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// DELETE /api/users/me/gpg-keys/{id}
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_delete_gpg_key_success(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, add_body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let key_id = add_body["id"].as_str().unwrap();

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/users/me/gpg-keys/{key_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify deleted
    let (status, _) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/users/me/gpg-keys/{key_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_delete_gpg_key_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, _) = helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/users/me/gpg-keys/{}", Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_delete_other_users_key_returns_not_found(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    // Admin adds a key
    let (status, add_body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let key_id = add_body["id"].as_str().unwrap();

    // Other user
    let (_, other_token) =
        helpers::create_user(&app, &admin_token, "other-user", "other@example.com").await;

    let (status, _) = helpers::delete_json(
        &app,
        &other_token,
        &format!("/api/users/me/gpg-keys/{key_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_add_duplicate_fingerprint_returns_conflict(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    // First add
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Duplicate
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_add_gpg_key_invalid_armor_returns_400(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({
            "public_key": "this is not a valid PGP key at all but it needs to be at least 100 characters long to pass validation"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_add_gpg_key_no_email_match_returns_400(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    // This key has email "other@example.com" which doesn't match admin's email
    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_MISMATCH_GPG_KEY }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "should reject mismatched email: {body}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_add_gpg_key_public_key_too_short_returns_400(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": "short" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_add_gpg_key_public_key_too_long_returns_400(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let long_key = "a".repeat(100_001);
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": long_key }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_list_gpg_keys_unauthenticated(pool: PgPool) {
    let (state, _admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, _) = helpers::get_json(&app, "", "/api/users/me/gpg-keys").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_add_gpg_key_unauthenticated(pool: PgPool) {
    let (state, _admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, _) = helpers::post_json(
        &app,
        "",
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_delete_gpg_key_unauthenticated(pool: PgPool) {
    let (state, _admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (status, _) = helpers::delete_json(
        &app,
        "",
        &format!("/api/users/me/gpg-keys/{}", Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Admin endpoint
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_admin_list_user_gpg_keys(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    // Add a key as admin
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Get admin user ID
    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE name = 'admin'")
        .fetch_one(&state.pool)
        .await
        .unwrap();

    // Admin lists their own keys via admin endpoint
    let (status, body) = helpers::get_json(
        &app,
        &admin_token,
        &format!("/api/admin/users/{admin_id}/gpg-keys"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_admin_list_gpg_keys_non_admin_denied(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (_, other_token) =
        helpers::create_user(&app, &admin_token, "other-user", "other@example.com").await;

    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE name = 'admin'")
        .fetch_one(&state.pool)
        .await
        .unwrap();

    let (status, _) = helpers::get_json(
        &app,
        &other_token,
        &format!("/api/admin/users/{admin_id}/gpg-keys"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Audit logging
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_add_gpg_key_creates_audit_log(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'gpg_key.add'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_delete_gpg_key_creates_audit_log(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    let (_, add_body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    let key_id = add_body["id"].as_str().unwrap();

    helpers::delete_json(
        &app,
        &admin_token,
        &format!("/api/users/me/gpg-keys/{key_id}"),
    )
    .await;

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'gpg_key.delete'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

// ---------------------------------------------------------------------------
// Isolation
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn test_list_gpg_keys_only_own_keys(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool).await;
    let app = helpers::test_router(state.clone());

    // Admin adds a key
    let (status, _) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Other user with matching email for the mismatch key
    let (_, other_token) =
        helpers::create_user(&app, &admin_token, "other-user", "other@example.com").await;

    // Other user adds the mismatch key (email matches their account)
    let (status, _) = helpers::post_json(
        &app,
        &other_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_MISMATCH_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Each user should only see their own keys
    let (_, admin_keys) = helpers::get_json(&app, &admin_token, "/api/users/me/gpg-keys").await;
    assert_eq!(admin_keys.as_array().unwrap().len(), 1);

    let (_, other_keys) = helpers::get_json(&app, &other_token, "/api/users/me/gpg-keys").await;
    assert_eq!(other_keys.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_add_gpg_key_max_50_limit(pool: PgPool) {
    let (state, admin_token) = helpers::test_state(pool.clone()).await;
    let app = helpers::test_router(state);

    // Insert 50 keys directly into DB
    let admin_row: (Uuid,) = sqlx::query_as("SELECT id FROM users WHERE name = 'admin'")
        .fetch_one(&pool)
        .await
        .unwrap();

    for i in 0..50 {
        sqlx::query(
            "INSERT INTO user_gpg_keys (user_id, key_id, fingerprint, public_key_armor, public_key_bytes, emails, can_sign)
             VALUES ($1, $2, $3, 'armor', '\\x00', ARRAY['admin@localhost'], true)",
        )
        .bind(admin_row.0)
        .bind(format!("FAKEKEYID{i:06}"))
        .bind(format!("FAKEFP{i:010}"))
        .execute(&pool)
        .await
        .unwrap();
    }

    // 51st key via API should fail
    let (status, body) = helpers::post_json(
        &app,
        &admin_token,
        "/api/users/me/gpg-keys",
        serde_json::json!({ "public_key": TEST_ED25519_GPG_KEY }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn test_get_gpg_key_unauthenticated(pool: PgPool) {
    let (state, _) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let fake_id = Uuid::new_v4();
    let (status, _) = helpers::get_json(
        &app,
        "bad-token",
        &format!("/api/users/me/gpg-keys/{fake_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn test_admin_list_gpg_keys_unauthenticated(pool: PgPool) {
    let (state, _) = helpers::test_state(pool).await;
    let app = helpers::test_router(state);

    let fake_user_id = Uuid::new_v4();
    let (status, _) = helpers::get_json(
        &app,
        "bad-token",
        &format!("/api/admin/users/{fake_user_id}/gpg-keys"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
