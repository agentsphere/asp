use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{password, token};
use crate::rbac::resolver;

use super::AgentRoleName;
use super::error::AgentError;

/// Result of creating an ephemeral agent identity.
pub struct AgentIdentity {
    pub user_id: Uuid,
    /// Raw API token (shown only once, passed to pod env var).
    pub api_token: String,
}

/// Create an ephemeral agent user, assign the requested agent role, compute
/// effective permissions (role ∩ spawner), and generate a scoped API token.
///
/// No delegation rows are created — the token's `scopes` column carries the
/// pre-computed permission set, and `project_id` / `scope_workspace_id` columns
/// enforce hard resource boundaries.
#[tracing::instrument(skip(pool, valkey), fields(%session_id, %spawner_id, %project_id, %workspace_id, %agent_role), err)]
pub async fn create_agent_identity(
    pool: &PgPool,
    valkey: &fred::clients::Pool,
    session_id: Uuid,
    spawner_id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
    agent_role: AgentRoleName,
) -> Result<AgentIdentity, AgentError> {
    let agent_user_id = Uuid::new_v4();
    let short_id = &session_id.to_string()[..8];
    let agent_name = format!("agent-{short_id}");

    // 1. Create agent user with a random password hash (cannot be used for login)
    let random_hash = password::hash_password(&format!("__agent_nologin_{agent_user_id}__"))
        .map_err(AgentError::Other)?;

    sqlx::query!(
        r#"
        INSERT INTO users (id, name, display_name, email, password_hash, is_active)
        VALUES ($1, $2, $3, $4, $5, true)
        "#,
        agent_user_id,
        agent_name,
        format!("Agent Session {short_id}"),
        format!("{agent_name}@agent.platform.local"),
        random_hash,
    )
    .execute(pool)
    .await?;

    // 2. Assign the REQUESTED agent role (e.g. "agent-dev"), not the generic "agent"
    let role_id = sqlx::query_scalar!(
        "SELECT id FROM roles WHERE name = $1",
        agent_role.db_role_name(),
    )
    .fetch_one(pool)
    .await?;

    let role_project_id = if agent_role.is_workspace_scoped() {
        None
    } else {
        Some(project_id)
    };

    sqlx::query!(
        "INSERT INTO user_roles (id, user_id, role_id, project_id) VALUES ($1, $2, $3, $4)",
        Uuid::new_v4(),
        agent_user_id,
        role_id,
        role_project_id,
    )
    .execute(pool)
    .await?;

    // 3. Compute effective permissions: role_perms ∩ spawner_perms
    let role_perms = resolver::role_permissions(pool, role_id).await?;
    let spawner_perms =
        resolver::effective_permissions(pool, valkey, spawner_id, Some(project_id)).await?;

    let effective: Vec<String> = role_perms
        .iter()
        .filter(|p| spawner_perms.contains(p))
        .map(|p| p.as_str().to_owned())
        .collect();

    // 4. Create SCOPED API token with hard boundaries
    let (raw_token, token_hash) = token::generate_api_token();
    let token_expires = Utc::now() + Duration::hours(24);

    let (scope_ws, scope_proj) = if agent_role.is_workspace_scoped() {
        (Some(workspace_id), None) // manager: workspace boundary only
    } else {
        (Some(workspace_id), Some(project_id)) // dev/ops/test/review: project boundary
    };

    sqlx::query!(
        r#"
        INSERT INTO api_tokens (user_id, name, token_hash, scopes, project_id, scope_workspace_id, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        agent_user_id,
        format!("agent-session-{session_id}"),
        token_hash,
        &effective,
        scope_proj,
        scope_ws,
        token_expires,
    )
    .execute(pool)
    .await?;

    tracing::info!(%agent_user_id, %session_id, role = %agent_role, perms = effective.len(), "agent identity created");

    Ok(AgentIdentity {
        user_id: agent_user_id,
        api_token: raw_token,
    })
}

/// Cleanup an agent identity: delete roles, tokens, sessions, deactivate user.
/// Called when a session finishes (completed, failed, or stopped).
#[tracing::instrument(skip(pool, valkey), fields(%agent_user_id), err)]
pub async fn cleanup_agent_identity(
    pool: &PgPool,
    valkey: &fred::clients::Pool,
    agent_user_id: Uuid,
) -> Result<(), AgentError> {
    // Delete role assignments for this agent user
    sqlx::query!("DELETE FROM user_roles WHERE user_id = $1", agent_user_id)
        .execute(pool)
        .await?;

    // Delete all API tokens for this agent user
    sqlx::query!("DELETE FROM api_tokens WHERE user_id = $1", agent_user_id)
        .execute(pool)
        .await?;

    // Delete all auth sessions for this agent user
    sqlx::query!(
        "DELETE FROM auth_sessions WHERE user_id = $1",
        agent_user_id
    )
    .execute(pool)
    .await?;

    // Deactivate the agent user
    sqlx::query!(
        "UPDATE users SET is_active = false WHERE id = $1",
        agent_user_id
    )
    .execute(pool)
    .await?;

    // Invalidate permission cache
    let _ = crate::rbac::resolver::invalidate_permissions(valkey, agent_user_id, None).await;

    tracing::info!(%agent_user_id, "agent identity cleaned up");
    Ok(())
}
