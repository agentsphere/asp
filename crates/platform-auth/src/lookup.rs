// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use sqlx::PgPool;
use uuid::Uuid;

use crate::token;
use platform_types::ApiError;

/// Row returned when looking up an API token.
pub struct TokenAuthLookup {
    pub user_id: Uuid,
    pub user_name: String,
    pub user_type: String,
    pub is_active: bool,
    pub name: String,
    pub scopes: Vec<String>,
    pub scope_project_id: Option<Uuid>,
    pub scope_workspace_id: Option<Uuid>,
}

/// Row returned when looking up a session.
pub struct SessionAuthLookup {
    pub user_id: Uuid,
    pub user_name: String,
    pub user_type: String,
    pub is_active: bool,
}

/// Look up an API token by its raw value. Updates `last_used_at` on success.
pub async fn lookup_api_token(
    pool: &PgPool,
    raw_token: &str,
) -> Result<Option<TokenAuthLookup>, ApiError> {
    let hash = token::hash_token(raw_token);

    let row = sqlx::query!(
        r#"SELECT u.id as user_id, u.name as user_name,
               u.user_type as "user_type!", u.is_active as "is_active!",
               t.name as "name!",
               t.scopes as "scopes!",
               t.project_id as scope_project_id,
               t.scope_workspace_id
        FROM api_tokens t
        JOIN users u ON u.id = t.user_id
        WHERE t.token_hash = $1
          AND (t.expires_at IS NULL OR t.expires_at > now())"#,
        &hash,
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let lookup = TokenAuthLookup {
        user_id: row.user_id,
        user_name: row.user_name,
        user_type: row.user_type,
        is_active: row.is_active,
        name: row.name,
        scopes: row.scopes,
        scope_project_id: row.scope_project_id,
        scope_workspace_id: row.scope_workspace_id,
    };

    // Fire-and-forget last_used_at update
    let pool = pool.clone();
    let hash = hash.clone();
    tokio::spawn(async move {
        let _ = sqlx::query!(
            "UPDATE api_tokens SET last_used_at = now() WHERE token_hash = $1",
            hash,
        )
        .execute(&pool)
        .await;
    });

    Ok(Some(lookup))
}

/// Look up a session by its raw cookie value.
pub async fn lookup_session(
    pool: &PgPool,
    raw_token: &str,
) -> Result<Option<SessionAuthLookup>, ApiError> {
    let hash = token::hash_token(raw_token);

    let row = sqlx::query!(
        r#"SELECT u.id as user_id, u.name as user_name,
               u.user_type as "user_type!", u.is_active as "is_active!"
        FROM auth_sessions s
        JOIN users u ON u.id = s.user_id
        WHERE s.token_hash = $1
          AND s.expires_at > now()"#,
        &hash,
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(SessionAuthLookup {
        user_id: row.user_id,
        user_name: row.user_name,
        user_type: row.user_type,
        is_active: row.is_active,
    }))
}
