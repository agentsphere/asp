// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Integration tests for `platform-auth` crate.
//!
//! These tests exercise auth functions against a real Postgres (via `#[sqlx::test]`)
//! and real Valkey (via `VALKEY_URL` env var).

use fred::interfaces::ClientLike;
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers — seed test data via raw SQL (no AppState / bootstrap needed)
// ---------------------------------------------------------------------------

async fn valkey_pool() -> fred::clients::Pool {
    let url = std::env::var("VALKEY_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
    // fred's URL parser chokes on redis://:password@host (empty username).
    // Normalize to redis://default:password@host which is equivalent.
    let url = url.replace("redis://:", "redis://default:");
    let config = fred::types::config::Config::from_url(&url).expect("invalid VALKEY_URL");
    let pool =
        fred::clients::Pool::new(config, None, None, None, 1).expect("valkey pool creation failed");
    pool.init().await.expect("valkey connection failed");
    pool
}

/// Insert a minimal test user. Returns `user_id`.
async fn seed_user(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    // Need a workspace first (projects requires workspace_id NOT NULL on projects,
    // but we just need the user itself for auth tests).
    sqlx::query(
        "INSERT INTO users (id, name, email, password_hash, user_type)
         VALUES ($1, $2, $3, 'not-a-real-hash', 'human')",
    )
    .bind(id)
    .bind(name)
    .bind(format!("{name}@test.local"))
    .execute(pool)
    .await
    .expect("seed user");
    id
}

/// Insert an API token for a user. Returns (`raw_token`, `token_hash`).
async fn seed_api_token(
    pool: &PgPool,
    user_id: Uuid,
    name: &str,
    scopes: &[String],
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> (String, String) {
    let (raw, hash) = platform_auth::token::generate_api_token();
    sqlx::query(
        "INSERT INTO api_tokens (user_id, name, token_hash, scopes, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(name)
    .bind(&hash)
    .bind(scopes)
    .bind(expires_at)
    .execute(pool)
    .await
    .expect("seed api_token");
    (raw, hash)
}

/// Insert an auth session for a user. Returns (`raw_token`, `token_hash`).
async fn seed_session(
    pool: &PgPool,
    user_id: Uuid,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> (String, String) {
    let (raw, hash) = platform_auth::token::generate_session_token();
    sqlx::query(
        "INSERT INTO auth_sessions (user_id, token_hash, expires_at)
         VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(&hash)
    .bind(expires_at)
    .execute(pool)
    .await
    .expect("seed auth_session");
    (raw, hash)
}

/// Seed a permission row. Returns `permission_id`.
async fn seed_permission(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    let parts: Vec<&str> = name.splitn(2, ':').collect();
    let (resource, action) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        (name, "read")
    };
    sqlx::query(
        "INSERT INTO permissions (id, name, resource, action)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(name)
    .bind(resource)
    .bind(action)
    .execute(pool)
    .await
    .expect("seed permission");
    id
}

/// Seed a role with given permissions. Returns `role_id`.
async fn seed_role(pool: &PgPool, name: &str, permission_ids: &[Uuid]) -> Uuid {
    let role_id = Uuid::new_v4();
    sqlx::query("INSERT INTO roles (id, name) VALUES ($1, $2)")
        .bind(role_id)
        .bind(name)
        .execute(pool)
        .await
        .expect("seed role");

    for perm_id in permission_ids {
        sqlx::query("INSERT INTO role_permissions (role_id, permission_id) VALUES ($1, $2)")
            .bind(role_id)
            .bind(perm_id)
            .execute(pool)
            .await
            .expect("seed role_permission");
    }
    role_id
}

/// Assign a role to a user (optionally project-scoped).
async fn assign_role(pool: &PgPool, user_id: Uuid, role_id: Uuid, project_id: Option<Uuid>) {
    sqlx::query("INSERT INTO user_roles (user_id, role_id, project_id) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(role_id)
        .bind(project_id)
        .execute(pool)
        .await
        .expect("assign role");
}

/// Create a workspace + member. Returns `workspace_id`.
async fn seed_workspace(pool: &PgPool, owner_id: Uuid, member_id: Uuid, role: &str) -> Uuid {
    let ws_id = Uuid::new_v4();
    let name = format!("ws-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO workspaces (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(ws_id)
        .bind(&name)
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("seed workspace");

    // Add owner as member
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(ws_id)
    .bind(owner_id)
    .execute(pool)
    .await
    .expect("seed workspace owner member");

    // Add the target member if different from owner
    if member_id != owner_id {
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)",
        )
        .bind(ws_id)
        .bind(member_id)
        .bind(role)
        .execute(pool)
        .await
        .expect("seed workspace member");
    }

    ws_id
}

/// Create a project in a workspace. Returns `project_id`.
async fn seed_project(pool: &PgPool, owner_id: Uuid, workspace_id: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    let name = format!("proj-{}", Uuid::new_v4());
    let slug = format!("slug-{}", Uuid::new_v4());
    sqlx::query(
        "INSERT INTO projects (id, owner_id, workspace_id, name, namespace_slug)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(owner_id)
    .bind(workspace_id)
    .bind(&name)
    .bind(&slug)
    .execute(pool)
    .await
    .expect("seed project");
    id
}

// ---------------------------------------------------------------------------
// API token lookup
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn lookup_api_token_returns_correct_fields(pool: PgPool) {
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;
    let scopes = vec!["project:read".to_string()];
    let (raw_token, _) = seed_api_token(&pool, user_id, "test-token", &scopes, None).await;

    let lookup = platform_auth::lookup_api_token(&pool, &raw_token)
        .await
        .expect("lookup should not error")
        .expect("token should be found");

    assert_eq!(lookup.user_id, user_id);
    assert_eq!(lookup.user_type, "human");
    assert!(lookup.is_active);
    assert_eq!(lookup.name, "test-token");
    assert_eq!(lookup.scopes, scopes);
    assert!(lookup.scope_project_id.is_none());
    assert!(lookup.scope_workspace_id.is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn lookup_api_token_expired_returns_none(pool: PgPool) {
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let (raw_token, _) = seed_api_token(&pool, user_id, "expired", &[], Some(past)).await;

    let result = platform_auth::lookup_api_token(&pool, &raw_token)
        .await
        .expect("should not error");
    assert!(result.is_none(), "expired token should not be found");
}

#[sqlx::test(migrations = "../../migrations")]
async fn lookup_api_token_wrong_token_returns_none(pool: PgPool) {
    let result = platform_auth::lookup_api_token(&pool, "plat_notarealtoken")
        .await
        .expect("should not error");
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// Session lookup
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn lookup_session_returns_correct_fields(pool: PgPool) {
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;
    let expires = chrono::Utc::now() + chrono::Duration::hours(24);
    let (raw_token, _) = seed_session(&pool, user_id, expires).await;

    let lookup = platform_auth::lookup_session(&pool, &raw_token)
        .await
        .expect("lookup should not error")
        .expect("session should be found");

    assert_eq!(lookup.user_id, user_id);
    assert_eq!(lookup.user_type, "human");
    assert!(lookup.is_active);
}

#[sqlx::test(migrations = "../../migrations")]
async fn lookup_session_expired_returns_none(pool: PgPool) {
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let (raw_token, _) = seed_session(&pool, user_id, past).await;

    let result = platform_auth::lookup_session(&pool, &raw_token)
        .await
        .expect("should not error");
    assert!(result.is_none(), "expired session should not be found");
}

// ---------------------------------------------------------------------------
// Permission resolution
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn effective_permissions_from_global_role(pool: PgPool) {
    let valkey = valkey_pool().await;
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;

    let perm_id = seed_permission(&pool, "project:read").await;
    let role_id = seed_role(&pool, &format!("role-{}", Uuid::new_v4()), &[perm_id]).await;
    assign_role(&pool, user_id, role_id, None).await;

    let perms = platform_auth::resolver::effective_permissions(&pool, &valkey, user_id, None)
        .await
        .expect("should resolve");

    assert!(
        perms.contains(&platform_types::Permission::ProjectRead),
        "should have project:read, got: {perms:?}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn effective_permissions_from_project_role(pool: PgPool) {
    let valkey = valkey_pool().await;
    let owner_id = seed_user(&pool, &format!("owner-{}", Uuid::new_v4())).await;
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;

    let ws_id = seed_workspace(&pool, owner_id, user_id, "member").await;
    let project_id = seed_project(&pool, owner_id, ws_id).await;

    let perm_id = seed_permission(&pool, "project:write").await;
    let role_id = seed_role(&pool, &format!("role-{}", Uuid::new_v4()), &[perm_id]).await;
    assign_role(&pool, user_id, role_id, Some(project_id)).await;

    let perms =
        platform_auth::resolver::effective_permissions(&pool, &valkey, user_id, Some(project_id))
            .await
            .expect("should resolve");

    assert!(perms.contains(&platform_types::Permission::ProjectWrite));
}

#[sqlx::test(migrations = "../../migrations")]
async fn effective_permissions_from_delegation(pool: PgPool) {
    let valkey = valkey_pool().await;
    let delegator_id = seed_user(&pool, &format!("delegator-{}", Uuid::new_v4())).await;
    let delegate_id = seed_user(&pool, &format!("delegate-{}", Uuid::new_v4())).await;

    let perm_id = seed_permission(&pool, "admin:config").await;

    sqlx::query(
        "INSERT INTO delegations (delegator_id, delegate_id, permission_id)
         VALUES ($1, $2, $3)",
    )
    .bind(delegator_id)
    .bind(delegate_id)
    .bind(perm_id)
    .execute(&pool)
    .await
    .expect("seed delegation");

    let perms = platform_auth::resolver::effective_permissions(&pool, &valkey, delegate_id, None)
        .await
        .expect("should resolve");

    assert!(perms.contains(&platform_types::Permission::AdminConfig));
}

#[sqlx::test(migrations = "../../migrations")]
async fn effective_permissions_revoked_delegation_excluded(pool: PgPool) {
    let valkey = valkey_pool().await;
    let delegator_id = seed_user(&pool, &format!("delegator-{}", Uuid::new_v4())).await;
    let delegate_id = seed_user(&pool, &format!("delegate-{}", Uuid::new_v4())).await;

    let perm_id = seed_permission(&pool, "admin:config").await;

    sqlx::query(
        "INSERT INTO delegations (delegator_id, delegate_id, permission_id, revoked_at)
         VALUES ($1, $2, $3, now())",
    )
    .bind(delegator_id)
    .bind(delegate_id)
    .bind(perm_id)
    .execute(&pool)
    .await
    .expect("seed revoked delegation");

    let perms = platform_auth::resolver::effective_permissions(&pool, &valkey, delegate_id, None)
        .await
        .expect("should resolve");

    assert!(
        !perms.contains(&platform_types::Permission::AdminConfig),
        "revoked delegation should not grant permission"
    );
}

// ---------------------------------------------------------------------------
// Permission cache + invalidation
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn effective_permissions_cached_in_valkey(pool: PgPool) {
    use fred::interfaces::KeysInterface;

    let valkey = valkey_pool().await;
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;

    let perm_id = seed_permission(&pool, "project:read").await;
    let role_id = seed_role(&pool, &format!("role-{}", Uuid::new_v4()), &[perm_id]).await;
    assign_role(&pool, user_id, role_id, None).await;

    // First call populates cache
    let _ = platform_auth::resolver::effective_permissions(&pool, &valkey, user_id, None)
        .await
        .unwrap();

    // Verify cache key exists
    let cache_key = format!("perms:{user_id}:global");
    let exists: bool = valkey.exists(&cache_key).await.unwrap();
    assert!(exists, "cache key should exist after first call");

    // Cleanup
    let _: () = valkey.del(&cache_key).await.unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn invalidate_permissions_clears_cache(pool: PgPool) {
    use fred::interfaces::KeysInterface;

    let valkey = valkey_pool().await;
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;

    // Seed and populate cache
    let perm_id = seed_permission(&pool, "project:read").await;
    let role_id = seed_role(&pool, &format!("role-{}", Uuid::new_v4()), &[perm_id]).await;
    assign_role(&pool, user_id, role_id, None).await;

    let _ = platform_auth::resolver::effective_permissions(&pool, &valkey, user_id, None)
        .await
        .unwrap();

    let cache_key = format!("perms:{user_id}:global");
    let exists_before: bool = valkey.exists(&cache_key).await.unwrap();
    assert!(exists_before, "cache should be populated");

    // Invalidate
    platform_auth::resolver::invalidate_permissions(&valkey, user_id, None)
        .await
        .expect("invalidation should succeed");

    let exists_after: bool = valkey.exists(&cache_key).await.unwrap();
    assert!(!exists_after, "cache should be cleared after invalidation");
}

// ---------------------------------------------------------------------------
// Scoped permission checks
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn has_permission_scoped_allows_matching_scope(pool: PgPool) {
    let valkey = valkey_pool().await;
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;

    let perm_id = seed_permission(&pool, "project:read").await;
    let role_id = seed_role(&pool, &format!("role-{}", Uuid::new_v4()), &[perm_id]).await;
    assign_role(&pool, user_id, role_id, None).await;

    let scopes = vec!["project:read".to_string()];
    let allowed = platform_auth::resolver::has_permission_scoped(
        &pool,
        &valkey,
        user_id,
        None,
        platform_types::Permission::ProjectRead,
        Some(&scopes),
    )
    .await
    .expect("should not error");

    assert!(allowed, "matching scope should allow");

    // Cleanup cache
    platform_auth::resolver::invalidate_permissions(&valkey, user_id, None)
        .await
        .ok();
}

#[sqlx::test(migrations = "../../migrations")]
async fn has_permission_scoped_denies_non_matching_scope(pool: PgPool) {
    let valkey = valkey_pool().await;
    let user_id = seed_user(&pool, &format!("u-{}", Uuid::new_v4())).await;

    let perm_id = seed_permission(&pool, "project:read").await;
    let role_id = seed_role(&pool, &format!("role-{}", Uuid::new_v4()), &[perm_id]).await;
    assign_role(&pool, user_id, role_id, None).await;

    // Token scopes only allow project:write — but user only has project:read
    let scopes = vec!["project:write".to_string()];
    let allowed = platform_auth::resolver::has_permission_scoped(
        &pool,
        &valkey,
        user_id,
        None,
        platform_types::Permission::ProjectRead,
        Some(&scopes),
    )
    .await
    .expect("should not error");

    assert!(!allowed, "non-matching scope should deny");

    // Cleanup cache
    platform_auth::resolver::invalidate_permissions(&valkey, user_id, None)
        .await
        .ok();
}

// ---------------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn check_rate_allows_under_threshold(_pool: PgPool) {
    let valkey = valkey_pool().await;
    let id = Uuid::new_v4().to_string();

    // First call should be well under the threshold
    let result = platform_auth::check_rate(&valkey, "test", &id, 10, 60).await;
    assert!(result.is_ok(), "should allow under threshold");
}

#[sqlx::test(migrations = "../../migrations")]
async fn check_rate_blocks_over_threshold(_pool: PgPool) {
    use fred::interfaces::KeysInterface;

    let valkey = valkey_pool().await;
    let id = Uuid::new_v4().to_string();
    let rate_key = format!("rate:test:{id}");

    // Pre-set the counter to threshold
    let _: () = valkey
        .set(&rate_key, 10i64, None, None, false)
        .await
        .unwrap();
    let _: () = valkey.expire(&rate_key, 300, None).await.unwrap();

    let result = platform_auth::check_rate(&valkey, "test", &id, 10, 60).await;
    assert!(result.is_err(), "should block over threshold");

    // Cleanup
    let _: () = valkey.del(&rate_key).await.unwrap();
}

// ---------------------------------------------------------------------------
// Workspace-derived permissions
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn workspace_member_gets_project_read(pool: PgPool) {
    let valkey = valkey_pool().await;
    let owner_id = seed_user(&pool, &format!("owner-{}", Uuid::new_v4())).await;
    let member_id = seed_user(&pool, &format!("member-{}", Uuid::new_v4())).await;

    let ws_id = seed_workspace(&pool, owner_id, member_id, "member").await;
    let project_id = seed_project(&pool, owner_id, ws_id).await;

    let perms =
        platform_auth::resolver::effective_permissions(&pool, &valkey, member_id, Some(project_id))
            .await
            .expect("should resolve");

    assert!(
        perms.contains(&platform_types::Permission::ProjectRead),
        "workspace member should get ProjectRead"
    );

    // Cleanup cache
    platform_auth::resolver::invalidate_permissions(&valkey, member_id, Some(project_id))
        .await
        .ok();
}

#[sqlx::test(migrations = "../../migrations")]
async fn workspace_admin_gets_project_write(pool: PgPool) {
    let valkey = valkey_pool().await;
    let owner_id = seed_user(&pool, &format!("owner-{}", Uuid::new_v4())).await;
    let admin_id = seed_user(&pool, &format!("admin-{}", Uuid::new_v4())).await;

    let ws_id = seed_workspace(&pool, owner_id, admin_id, "admin").await;
    let project_id = seed_project(&pool, owner_id, ws_id).await;

    let perms =
        platform_auth::resolver::effective_permissions(&pool, &valkey, admin_id, Some(project_id))
            .await
            .expect("should resolve");

    assert!(perms.contains(&platform_types::Permission::ProjectRead));
    assert!(
        perms.contains(&platform_types::Permission::ProjectWrite),
        "workspace admin should get ProjectWrite"
    );

    // Cleanup cache
    platform_auth::resolver::invalidate_permissions(&valkey, admin_id, Some(project_id))
        .await
        .ok();
}

// ---------------------------------------------------------------------------
// Role permissions query
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn role_permissions_returns_assigned_permissions(pool: PgPool) {
    let perm1 = seed_permission(&pool, "project:read").await;
    let perm2 = seed_permission(&pool, "project:write").await;
    let role_id = seed_role(&pool, &format!("role-{}", Uuid::new_v4()), &[perm1, perm2]).await;

    let perms = platform_auth::resolver::role_permissions(&pool, role_id)
        .await
        .expect("should resolve");

    assert_eq!(perms.len(), 2);
    assert!(perms.contains(&platform_types::Permission::ProjectRead));
    assert!(perms.contains(&platform_types::Permission::ProjectWrite));
}
