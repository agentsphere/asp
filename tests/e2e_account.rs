//! E2E tests for account settings: password change and passkey management.
//! Multi-step journeys using `e2e_helpers` with real infrastructure.

mod e2e_helpers;

use axum::http::StatusCode;
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;
use webauthn_authenticator_rs::AuthenticatorBackend;
use webauthn_authenticator_rs::softpasskey::SoftPasskey;
use webauthn_rs::prelude::{
    Base64UrlSafeData, CreationChallengeResponse, RegisterPublicKeyCredential,
    RequestChallengeResponse,
};
use webauthn_rs_proto::AllowCredentials;

// ---------------------------------------------------------------------------
// Passkey ceremony helpers (adapted from passkey_integration.rs for e2e)
// ---------------------------------------------------------------------------

async fn register_passkey_ceremony(
    app: &axum::Router,
    token: &str,
    authenticator: &mut SoftPasskey,
) -> Uuid {
    let (status, body) = e2e_helpers::post_json(
        app,
        token,
        "/api/auth/passkeys/register/begin",
        serde_json::json!({"name": "E2E Key"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "begin_register failed: {body}");

    let ccr: CreationChallengeResponse =
        serde_json::from_value(body).expect("parse CreationChallengeResponse");

    let origin = Url::parse("http://localhost:8080").unwrap();
    let reg_response: RegisterPublicKeyCredential = authenticator
        .perform_register(origin, ccr.public_key, 60000)
        .expect("SoftPasskey perform_register");

    let reg_json = serde_json::to_value(&reg_response).unwrap();
    let (status, body) =
        e2e_helpers::post_json(app, token, "/api/auth/passkeys/register/complete", reg_json).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "complete_register failed: {body}"
    );

    Uuid::parse_str(body["id"].as_str().unwrap()).unwrap()
}

async fn login_passkey_ceremony(
    app: &axum::Router,
    pool: &PgPool,
    user_id: Uuid,
    authenticator: &mut SoftPasskey,
) -> serde_json::Value {
    let (status, body) = e2e_helpers::post_json(
        app,
        "",
        "/api/auth/passkeys/login/begin",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "begin_login failed: {body}");
    let challenge_id = body["challenge_id"].as_str().unwrap().to_string();

    let mut rcr: RequestChallengeResponse =
        serde_json::from_value(body["challenge"].clone()).expect("parse RequestChallengeResponse");

    let cred_bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT credential_id FROM passkey_credentials WHERE user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("credential should exist in DB");

    rcr.public_key.allow_credentials = vec![AllowCredentials {
        type_: "public-key".to_string(),
        id: Base64UrlSafeData::from(cred_bytes),
        transports: None,
    }];

    let origin = Url::parse("http://localhost:8080").unwrap();
    let auth_response = authenticator
        .perform_auth(origin, rcr.public_key, 60000)
        .expect("SoftPasskey perform_auth");

    let (status, body) = e2e_helpers::post_json(
        app,
        "",
        "/api/auth/passkeys/login/complete",
        serde_json::json!({
            "challenge_id": challenge_id,
            "credential": auth_response,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "complete_login failed: {body}");
    body
}

// ---------------------------------------------------------------------------
// Test 1: Password change journey
// ---------------------------------------------------------------------------

#[ignore = "requires Kind cluster"]
#[sqlx::test(migrations = "./migrations")]
async fn password_change_journey(pool: PgPool) {
    let (state, admin_token) = e2e_helpers::e2e_state(pool).await;
    let app = e2e_helpers::test_router(state.clone());

    // 1. Create a user with known password
    let name = format!("pw-user-{}", Uuid::new_v4().as_simple());
    let email = format!("{name}@test.com");
    let old_password = "testpass123";
    let new_password = "newpassword456";

    let (status, body) = e2e_helpers::post_json(
        &app,
        &admin_token,
        "/api/users",
        serde_json::json!({
            "name": name,
            "email": email,
            "password": old_password,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create user failed: {body}");
    let user_id = body["id"].as_str().unwrap().to_string();

    // Login as the user
    let (status, login_body) = e2e_helpers::post_json(
        &app,
        "",
        "/api/auth/login",
        serde_json::json!({ "name": name, "password": old_password }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login failed: {login_body}");
    let user_token = login_body["token"].as_str().unwrap().to_string();

    // 2. Change password
    let (status, body) = e2e_helpers::patch_json(
        &app,
        &user_token,
        &format!("/api/users/{user_id}"),
        serde_json::json!({ "password": new_password }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "change password failed: {body}");

    // 3. Login with old password should fail
    let (status, _) = e2e_helpers::post_json(
        &app,
        "",
        "/api/auth/login",
        serde_json::json!({ "name": name, "password": old_password }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "old password should not work"
    );

    // 4. Login with new password should succeed
    let (status, body) = e2e_helpers::post_json(
        &app,
        "",
        "/api/auth/login",
        serde_json::json!({ "name": name, "password": new_password }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "new password login failed: {body}");
}

// ---------------------------------------------------------------------------
// Test 2: Passkey register, list, rename, delete journey
// ---------------------------------------------------------------------------

#[ignore = "requires Kind cluster"]
#[sqlx::test(migrations = "./migrations")]
async fn passkey_register_list_rename_delete_journey(pool: PgPool) {
    let (state, admin_token) = e2e_helpers::e2e_state(pool.clone()).await;
    let app = e2e_helpers::test_router(state.clone());

    // Create a user
    let (_user_id, user_token) =
        e2e_helpers::create_user(&app, &admin_token, "pk-crud-e2e", "pkcrud@test.com").await;

    // 1. Register first passkey
    let mut auth1 = SoftPasskey::new(true);
    let cred1_id = register_passkey_ceremony(&app, &user_token, &mut auth1).await;

    // 2. List passkeys — verify 1 result
    let (status, body) = e2e_helpers::get_json(&app, &user_token, "/api/auth/passkeys").await;
    assert_eq!(status, StatusCode::OK);
    let keys = body.as_array().unwrap();
    assert_eq!(keys.len(), 1, "should have 1 passkey");
    assert_eq!(keys[0]["id"], cred1_id.to_string());

    // 3. Rename the passkey
    let (status, _) = e2e_helpers::patch_json(
        &app,
        &user_token,
        &format!("/api/auth/passkeys/{cred1_id}"),
        serde_json::json!({"name": "My YubiKey"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Verify name changed
    let (_, body) = e2e_helpers::get_json(&app, &user_token, "/api/auth/passkeys").await;
    assert_eq!(body[0]["name"], "My YubiKey");

    // 4. Register second passkey
    let mut auth2 = SoftPasskey::new(true);
    let _cred2_id = register_passkey_ceremony(&app, &user_token, &mut auth2).await;

    // 5. List — verify 2
    let (_, body) = e2e_helpers::get_json(&app, &user_token, "/api/auth/passkeys").await;
    assert_eq!(body.as_array().unwrap().len(), 2, "should have 2 passkeys");

    // 6. Delete first passkey
    let (status, _) =
        e2e_helpers::delete_json(&app, &user_token, &format!("/api/auth/passkeys/{cred1_id}"))
            .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 7. List — verify 1 remaining
    let (_, body) = e2e_helpers::get_json(&app, &user_token, "/api/auth/passkeys").await;
    let keys = body.as_array().unwrap();
    assert_eq!(keys.len(), 1, "should have 1 passkey after delete");
    assert_ne!(
        keys[0]["id"].as_str().unwrap(),
        cred1_id.to_string(),
        "deleted key should be gone"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Passkey login journey
// ---------------------------------------------------------------------------

#[ignore = "requires Kind cluster"]
#[sqlx::test(migrations = "./migrations")]
async fn passkey_login_journey(pool: PgPool) {
    let (state, admin_token) = e2e_helpers::e2e_state(pool.clone()).await;
    let app = e2e_helpers::test_router(state.clone());

    // 1. Create user and login with password
    let (user_id, user_token) =
        e2e_helpers::create_user(&app, &admin_token, "pk-login-e2e", "pklogin@test.com").await;

    // 2. Register passkey
    let mut authenticator = SoftPasskey::new(true);
    let _cred_id = register_passkey_ceremony(&app, &user_token, &mut authenticator).await;

    // 3. Login via passkey ceremony (simulating logout + re-login)
    let body = login_passkey_ceremony(&app, &state.pool, user_id, &mut authenticator).await;

    let passkey_token = body["token"].as_str().unwrap();
    assert!(!passkey_token.is_empty(), "should get a session token");

    // 4. Use returned token to call GET /api/auth/me
    let (status, me) = e2e_helpers::get_json(&app, passkey_token, "/api/auth/me").await;
    assert_eq!(status, StatusCode::OK, "auth/me with passkey token failed");
    assert_eq!(me["name"].as_str().unwrap(), "pk-login-e2e");
}

// ---------------------------------------------------------------------------
// Test 4: Password change does not break passkey login
// ---------------------------------------------------------------------------

#[ignore = "requires Kind cluster"]
#[sqlx::test(migrations = "./migrations")]
async fn password_change_does_not_break_passkey_login(pool: PgPool) {
    let (state, admin_token) = e2e_helpers::e2e_state(pool.clone()).await;
    let app = e2e_helpers::test_router(state.clone());

    // 1. Create user
    let name = format!("pk-pw-e2e-{}", Uuid::new_v4().as_simple());
    let email = format!("{name}@test.com");
    let (status, body) = e2e_helpers::post_json(
        &app,
        &admin_token,
        "/api/users",
        serde_json::json!({
            "name": name,
            "email": email,
            "password": "oldpassword123",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let user_id = Uuid::parse_str(body["id"].as_str().unwrap()).unwrap();

    // Login
    let (_, login_body) = e2e_helpers::post_json(
        &app,
        "",
        "/api/auth/login",
        serde_json::json!({ "name": name, "password": "oldpassword123" }),
    )
    .await;
    let user_token = login_body["token"].as_str().unwrap().to_string();

    // 2. Register passkey
    let mut authenticator = SoftPasskey::new(true);
    let _cred_id = register_passkey_ceremony(&app, &user_token, &mut authenticator).await;

    // 3. Change password
    let (status, _) = e2e_helpers::patch_json(
        &app,
        &user_token,
        &format!("/api/users/{user_id}"),
        serde_json::json!({ "password": "newpassword789" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "password change failed");

    // 4. Login via passkey should still work
    let body = login_passkey_ceremony(&app, &state.pool, user_id, &mut authenticator).await;
    let pk_token = body["token"].as_str().unwrap();

    let (status, me) = e2e_helpers::get_json(&app, pk_token, "/api/auth/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["name"].as_str().unwrap(), name);
}
