// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! [`RegistryCredentialProvider`] implementation backed by Postgres + K8s.
//!
//! Wraps the existing [`create_pull_secret`](crate::pull_secret::create_pull_secret)
//! free function and scoped-token creation logic.

use sqlx::PgPool;
use uuid::Uuid;

use platform_types::RegistryCredentialProvider;

use crate::pull_secret;

/// Provides registry credentials (pull secrets, scoped tokens) for pipeline
/// and agent pods.
pub struct RegistryCredentials<'a> {
    pool: &'a PgPool,
    registry_url: &'a str,
}

impl<'a> RegistryCredentials<'a> {
    pub fn new(pool: &'a PgPool, registry_url: &'a str) -> Self {
        Self { pool, registry_url }
    }
}

impl RegistryCredentialProvider for RegistryCredentials<'_> {
    async fn ensure_pull_secret(
        &self,
        kube: &kube::Client,
        ns: &str,
        project_id: Uuid,
    ) -> anyhow::Result<()> {
        // Look up the project owner to create a token on their behalf.
        let owner_id: Uuid =
            sqlx::query_scalar("SELECT owner_id FROM projects WHERE id = $1 AND is_active = true")
                .bind(project_id)
                .fetch_optional(self.pool)
                .await?
                .ok_or_else(|| anyhow::anyhow!("project not found: {project_id}"))?;

        let _result = pull_secret::create_pull_secret(
            self.pool,
            kube,
            self.registry_url,
            owner_id,
            ns,
            "platform.io/project",
            &project_id.to_string(),
        )
        .await?;

        Ok(())
    }

    async fn ensure_scoped_tokens(&self, project_id: Uuid, scope: &str) -> anyhow::Result<()> {
        // Create a scoped API token for registry access.
        let owner_id: Uuid =
            sqlx::query_scalar("SELECT owner_id FROM projects WHERE id = $1 AND is_active = true")
                .bind(project_id)
                .fetch_optional(self.pool)
                .await?
                .ok_or_else(|| anyhow::anyhow!("project not found: {project_id}"))?;

        let (raw_token, token_hash) = platform_auth::token::generate_api_token();
        let _ = raw_token; // Token is stored in DB; callers retrieve it via hash lookup.

        sqlx::query(
            "INSERT INTO api_tokens (id, user_id, name, token_hash, expires_at, registry_tag_pattern)
             VALUES ($1, $2, $3, $4, now() + interval '1 hour', $5)",
        )
        .bind(Uuid::new_v4())
        .bind(owner_id)
        .bind(format!("registry-scope-{scope}"))
        .bind(&token_hash)
        .bind(scope)
        .execute(self.pool)
        .await?;

        tracing::debug!(%project_id, %scope, "ensured scoped registry token");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_is_constructible() {
        fn make(pool: &PgPool) -> RegistryCredentials<'_> {
            RegistryCredentials::new(pool, "registry.example.com:5000")
        }
        let _ = make as fn(&PgPool) -> RegistryCredentials<'_>;
    }
}
