// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Integration tests for `platform-types` crate.
//!
//! Tests audit logging, Postgres pool creation, and Valkey cache operations
//! against real infrastructure (via `#[sqlx::test]` and `VALKEY_URL`).

use fred::interfaces::ClientLike;
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn valkey_pool() -> fred::clients::Pool {
    let url = std::env::var("VALKEY_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
    let url = url.replace("redis://:", "redis://default:");
    let config = fred::types::config::Config::from_url(&url).expect("invalid VALKEY_URL");
    let pool =
        fred::clients::Pool::new(config, None, None, None, 1).expect("valkey pool creation failed");
    pool.init().await.expect("valkey connection failed");
    pool
}

/// Seed a minimal user for audit tests. Returns `user_id`.
async fn seed_user(pool: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    let name = format!("u-{id}");
    sqlx::query(
        "INSERT INTO users (id, name, email, password_hash, user_type)
         VALUES ($1, $2, $3, 'not-a-real-hash', 'human')",
    )
    .bind(id)
    .bind(&name)
    .bind(format!("{name}@test.local"))
    .execute(pool)
    .await
    .expect("seed user");
    id
}

// ---------------------------------------------------------------------------
// audit.rs
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../../migrations")]
async fn send_audit_writes_to_db(pool: PgPool) {
    let user_id = seed_user(&pool).await;

    let audit_log = platform_types::AuditLog::new(pool.clone());
    platform_types::send_audit(
        &audit_log,
        platform_types::AuditEntry {
            actor_id: user_id,
            actor_name: "test-user".into(),
            action: "test.action".into(),
            resource: "test-resource".into(),
            resource_id: None,
            project_id: None,
            detail: Some(serde_json::json!({"key": "value"})),
            ip_addr: None,
        },
    );

    // send_audit spawns a tokio task — poll until the entry appears (max 2s)
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log WHERE actor_id = $1 AND action = 'test.action'",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("query audit_log");
        if count.0 > 0 {
            return; // success
        }
    }
    panic!("audit log entry did not appear within 2s");
}

#[sqlx::test(migrations = "../../../migrations")]
async fn send_audit_with_ip_addr(pool: PgPool) {
    let user_id = seed_user(&pool).await;

    let audit_log = platform_types::AuditLog::new(pool.clone());
    platform_types::send_audit(
        &audit_log,
        platform_types::AuditEntry {
            actor_id: user_id,
            actor_name: "test-user".into(),
            action: "test.ip_action".into(),
            resource: "test-resource".into(),
            resource_id: None,
            project_id: None,
            detail: None,
            ip_addr: Some("192.168.1.1".into()),
        },
    );

    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log WHERE actor_id = $1 AND action = 'test.ip_action'",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("query audit_log");
        if count.0 > 0 {
            return;
        }
    }
    panic!("audit log entry with IP did not appear within 2s");
}

// ---------------------------------------------------------------------------
// pool.rs
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../../migrations")]
async fn pg_connect_success(_pool: PgPool) {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let result = platform_types::pool::pg_connect(&url, 2, 5).await;
    assert!(result.is_ok(), "pg_connect failed: {result:?}");
}

// ---------------------------------------------------------------------------
// valkey.rs
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../../migrations")]
async fn valkey_connect_success(_pool: PgPool) {
    let url = std::env::var("VALKEY_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
    let result = platform_types::valkey::connect(&url, 1).await;
    assert!(result.is_ok(), "valkey connect failed: {result:?}");
}

#[sqlx::test(migrations = "../../../migrations")]
async fn get_cached_returns_none_for_missing_key(_pool: PgPool) {
    let valkey = valkey_pool().await;
    let key = format!("types-test-missing-{}", Uuid::new_v4());

    let result: Option<String> = platform_types::valkey::get_cached(&valkey, &key).await;
    assert!(result.is_none(), "missing key should return None");
}

#[sqlx::test(migrations = "../../../migrations")]
async fn get_cached_malformed_json_returns_none(_pool: PgPool) {
    use fred::interfaces::KeysInterface;

    let valkey = valkey_pool().await;
    let key = format!("types-test-malformed-{}", Uuid::new_v4());

    // Write raw invalid JSON
    let _: () = valkey
        .set(&key, "not-valid-json{{{", None, None, false)
        .await
        .unwrap();

    let result: Option<Vec<String>> = platform_types::valkey::get_cached(&valkey, &key).await;
    assert!(
        result.is_none(),
        "malformed JSON should return None (deserialization error path)"
    );

    // Cleanup
    let _: () = valkey.del(&key).await.unwrap();
}

#[sqlx::test(migrations = "../../../migrations")]
async fn set_cached_and_get_cached_roundtrip(_pool: PgPool) {
    use fred::interfaces::KeysInterface;

    let valkey = valkey_pool().await;
    let key = format!("types-test-roundtrip-{}", Uuid::new_v4());

    let data = vec!["hello".to_string(), "world".to_string()];
    platform_types::valkey::set_cached(&valkey, &key, &data, 60)
        .await
        .expect("set_cached should succeed");

    let result: Option<Vec<String>> = platform_types::valkey::get_cached(&valkey, &key).await;
    assert_eq!(result, Some(data));

    // Cleanup
    let _: () = valkey.del(&key).await.unwrap();
}

#[sqlx::test(migrations = "../../../migrations")]
async fn invalidate_removes_key(_pool: PgPool) {
    use fred::interfaces::KeysInterface;

    let valkey = valkey_pool().await;
    let key = format!("types-test-invalidate-{}", Uuid::new_v4());

    // Set a key
    platform_types::valkey::set_cached(&valkey, &key, &"test-value", 60)
        .await
        .unwrap();

    // Verify it exists
    let exists: bool = valkey.exists(&key).await.unwrap();
    assert!(exists, "key should exist before invalidation");

    // Invalidate
    platform_types::valkey::invalidate(&valkey, &key)
        .await
        .expect("invalidate should succeed");

    // Verify it's gone
    let exists: bool = valkey.exists(&key).await.unwrap();
    assert!(!exists, "key should be gone after invalidation");
}

#[sqlx::test(migrations = "../../../migrations")]
async fn invalidate_pattern_removes_matching_keys(_pool: PgPool) {
    use fred::interfaces::KeysInterface;

    let valkey = valkey_pool().await;
    let prefix = format!("types-test-pattern-{}", Uuid::new_v4());
    let key1 = format!("{prefix}:a");
    let key2 = format!("{prefix}:b");
    let key3 = format!("{prefix}:c");

    // Set multiple keys
    for key in [&key1, &key2, &key3] {
        platform_types::valkey::set_cached(&valkey, key, &"v", 60)
            .await
            .unwrap();
    }

    // Verify they exist
    for key in [&key1, &key2, &key3] {
        let exists: bool = valkey.exists(key).await.unwrap();
        assert!(exists, "key {key} should exist");
    }

    // Invalidate by pattern
    platform_types::valkey::invalidate_pattern(&valkey, &format!("{prefix}:*"))
        .await
        .expect("invalidate_pattern should succeed");

    // Verify all gone
    for key in [&key1, &key2, &key3] {
        let exists: bool = valkey.exists(key).await.unwrap();
        assert!(
            !exists,
            "key {key} should be gone after pattern invalidation"
        );
    }
}
