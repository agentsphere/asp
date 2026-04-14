// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Workspace membership checking backed by Postgres.

use sqlx::PgPool;
use uuid::Uuid;

use platform_types::WorkspaceMembershipChecker;

/// Checks workspace membership via the `workspace_members` table.
pub struct PgWorkspaceMembershipChecker<'a> {
    pool: &'a PgPool,
}

impl<'a> PgWorkspaceMembershipChecker<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }
}

impl WorkspaceMembershipChecker for PgWorkspaceMembershipChecker<'_> {
    async fn is_member(&self, workspace_id: Uuid, user_id: Uuid) -> anyhow::Result<bool> {
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS(
                SELECT 1 FROM workspace_members
                WHERE workspace_id = $1 AND user_id = $2
            ) as "exists!""#,
            workspace_id,
            user_id
        )
        .fetch_one(self.pool)
        .await?;
        Ok(exists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checker_is_constructible() {
        // Verify the struct can be instantiated (no runtime test without DB).
        // Integration tests in tests/ cover actual DB queries.
        fn make_checker(pool: &PgPool) -> PgWorkspaceMembershipChecker<'_> {
            PgWorkspaceMembershipChecker::new(pool)
        }
        let _ = make_checker as fn(&PgPool) -> PgWorkspaceMembershipChecker<'_>;
    }
}
