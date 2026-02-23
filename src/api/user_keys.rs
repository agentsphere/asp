use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::audit::{AuditEntry, write_audit};
use crate::auth::middleware::AuthUser;
use crate::error::ApiError;
use crate::secrets::{engine, user_keys};
use crate::store::AppState;
use crate::validation;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SetProviderKeyRequest {
    pub api_key: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_master_key(state: &AppState) -> Result<[u8; 32], ApiError> {
    let hex_str = state
        .config
        .master_key
        .as_deref()
        .ok_or_else(|| ApiError::ServiceUnavailable("secrets engine not configured".into()))?;
    engine::parse_master_key(hex_str).map_err(|e| {
        tracing::error!(error = %e, "invalid master key configuration");
        ApiError::ServiceUnavailable("secrets engine misconfigured".into())
    })
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/users/me/provider-keys", get(list_provider_keys))
        .route(
            "/api/users/me/provider-keys/{provider}",
            axum::routing::put(set_provider_key).delete(delete_provider_key),
        )
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// PUT /api/users/me/provider-keys/{provider}
async fn set_provider_key(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(provider): Path<String>,
    Json(body): Json<SetProviderKeyRequest>,
) -> Result<impl IntoResponse, ApiError> {
    validation::check_name(&provider)?;
    validation::check_length("api_key", &body.api_key, 10, 500)?;

    let master_key = get_master_key(&state)?;

    user_keys::set_user_key(
        &state.pool,
        &master_key,
        auth.user_id,
        &provider,
        &body.api_key,
    )
    .await
    .map_err(ApiError::Internal)?;

    write_audit(
        &state.pool,
        &AuditEntry {
            actor_id: auth.user_id,
            actor_name: &auth.user_name,
            action: "provider_key.set",
            resource: "provider_key",
            resource_id: None,
            project_id: None,
            detail: Some(serde_json::json!({ "provider": provider })),
            ip_addr: auth.ip_addr.as_deref(),
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/users/me/provider-keys
async fn list_provider_keys(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<user_keys::ProviderKeyMetadata>>, ApiError> {
    let keys = user_keys::list_user_keys(&state.pool, auth.user_id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(keys))
}

/// DELETE /api/users/me/provider-keys/{provider}
async fn delete_provider_key(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(provider): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    validation::check_name(&provider)?;

    let deleted = user_keys::delete_user_key(&state.pool, auth.user_id, &provider)
        .await
        .map_err(ApiError::Internal)?;

    if !deleted {
        return Err(ApiError::NotFound("provider key".into()));
    }

    write_audit(
        &state.pool,
        &AuditEntry {
            actor_id: auth.user_id,
            actor_name: &auth.user_name,
            action: "provider_key.delete",
            resource: "provider_key",
            resource_id: None,
            project_id: None,
            detail: Some(serde_json::json!({ "provider": provider })),
            ip_addr: auth.ip_addr.as_deref(),
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}
