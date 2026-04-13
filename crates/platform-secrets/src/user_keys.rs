// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::engine;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ProviderKeyMetadata {
    pub provider: String,
    pub key_suffix: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

/// Store (or replace) a provider API key for a user. The key is encrypted with
/// AES-256-GCM before being written to the database.
#[tracing::instrument(skip(pool, master_key, api_key), fields(%user_id, %provider), err)]
pub async fn set_user_key(
    pool: &PgPool,
    master_key: &[u8; 32],
    user_id: Uuid,
    provider: &str,
    api_key: &str,
) -> anyhow::Result<()> {
    let encrypted = engine::encrypt(api_key.as_bytes(), master_key)?;
    let suffix = key_suffix(api_key);

    sqlx::query!(
        r#"
        INSERT INTO user_provider_keys (user_id, provider, encrypted_key, key_suffix)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (user_id, provider)
        DO UPDATE SET
            encrypted_key = EXCLUDED.encrypted_key,
            key_suffix    = EXCLUDED.key_suffix,
            updated_at    = now()
        "#,
        user_id,
        provider,
        encrypted,
        suffix,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Decrypt and return a user's provider key. Returns `None` if not set.
#[tracing::instrument(skip(pool, master_key), fields(%user_id, %provider), err)]
pub async fn get_user_key(
    pool: &PgPool,
    master_key: &[u8; 32],
    user_id: Uuid,
    provider: &str,
) -> anyhow::Result<Option<String>> {
    let row = sqlx::query!(
        "SELECT encrypted_key FROM user_provider_keys WHERE user_id = $1 AND provider = $2",
        user_id,
        provider,
    )
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => {
            let plaintext = engine::decrypt(&r.encrypted_key, master_key, None)?;
            let key = String::from_utf8(plaintext)
                .map_err(|e| anyhow::anyhow!("provider key is not valid UTF-8: {e}"))?;
            Ok(Some(key))
        }
        None => Ok(None),
    }
}

/// Delete a user's provider key. Returns whether a row was deleted.
#[tracing::instrument(skip(pool), fields(%user_id, %provider), err)]
pub async fn delete_user_key(pool: &PgPool, user_id: Uuid, provider: &str) -> anyhow::Result<bool> {
    let result = sqlx::query!(
        "DELETE FROM user_provider_keys WHERE user_id = $1 AND provider = $2",
        user_id,
        provider,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// List all provider key metadata for a user. Never returns decrypted values.
pub async fn list_user_keys(
    pool: &PgPool,
    user_id: Uuid,
) -> anyhow::Result<Vec<ProviderKeyMetadata>> {
    let rows = sqlx::query!(
        r#"
        SELECT provider, key_suffix, created_at, updated_at
        FROM user_provider_keys
        WHERE user_id = $1
        ORDER BY provider
        "#,
        user_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ProviderKeyMetadata {
            provider: r.provider,
            key_suffix: r.key_suffix,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the last 4 characters of a key for display (e.g. "...xK7q").
fn key_suffix(key: &str) -> String {
    if key.len() >= 4 {
        format!("...{}", &key[key.len() - 4..])
    } else {
        format!("...{key}")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_normal_key() {
        assert_eq!(key_suffix("sk-ant-api03-abcdefghijklmnop"), "...mnop");
    }

    #[test]
    fn suffix_short_key() {
        assert_eq!(key_suffix("abc"), "...abc");
    }

    #[test]
    fn suffix_exactly_4_chars() {
        assert_eq!(key_suffix("abcd"), "...abcd");
    }

    #[test]
    fn suffix_long_key() {
        let key = "a".repeat(200);
        assert_eq!(key_suffix(&key), "...aaaa");
    }
}
