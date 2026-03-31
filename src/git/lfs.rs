// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::rbac::{Permission, resolver};
use crate::store::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LfsBatchRequest {
    pub operation: String,
    #[allow(dead_code)] // protocol field, validated but not used
    pub transfers: Option<Vec<String>>,
    pub objects: Vec<LfsObject>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LfsObject {
    pub oid: String,
    pub size: i64,
}

#[derive(Debug, Serialize)]
pub struct LfsBatchResponse {
    pub transfer: String,
    pub objects: Vec<LfsObjectResponse>,
}

#[derive(Debug, Serialize)]
pub struct LfsObjectResponse {
    pub oid: String,
    pub size: i64,
    pub actions: LfsActions,
}

#[derive(Debug, Serialize)]
pub struct LfsActions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload: Option<LfsAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download: Option<LfsAction>,
}

#[derive(Debug, Serialize)]
pub struct LfsAction {
    pub href: String,
    pub header: HashMap<String, String>,
    pub expires_in: i64,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new().route("/{owner}/{repo}/info/lfs/objects/batch", post(batch))
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `POST /:owner/:repo/info/lfs/objects/batch`
///
/// Git LFS batch API. Returns presigned `MinIO` URLs for upload/download.
#[tracing::instrument(skip(state, body), fields(%owner, %repo), err)]
async fn batch(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<LfsBatchRequest>,
) -> Result<Json<LfsBatchResponse>, ApiError> {
    // Authenticate via Basic Auth (same as smart HTTP)
    let git_user = super::smart_http::authenticate_basic(&headers, &state.pool).await?;

    // Resolve project
    let project =
        super::smart_http::resolve_project(&state.pool, &state.config, &owner, &repo).await?;

    // Enforce hard project boundary from API token
    if let Some(boundary_pid) = git_user.boundary_project_id
        && boundary_pid != project.project_id
    {
        return Err(ApiError::NotFound("repository".into()));
    }

    // Enforce hard workspace boundary from API token
    if let Some(scope_wid) = git_user.boundary_workspace_id {
        let in_workspace = sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1 AND workspace_id = $2 AND is_active = true) as "exists!: bool""#,
            project.project_id, scope_wid,
        )
        .fetch_one(&state.pool)
        .await?;
        if !in_workspace {
            return Err(ApiError::NotFound("repository".into()));
        }
    }

    // Check permission: download = ProjectRead, upload = ProjectWrite
    let required_perm = match body.operation.as_str() {
        "download" => Permission::ProjectRead,
        "upload" => Permission::ProjectWrite,
        _ => return Err(ApiError::BadRequest("invalid operation".into())),
    };

    // A8: Use has_permission_scoped to enforce API token scopes
    let allowed = resolver::has_permission_scoped(
        &state.pool,
        &state.valkey,
        git_user.user_id,
        Some(project.project_id),
        required_perm,
        git_user.token_scopes.as_deref(),
    )
    .await
    .map_err(ApiError::Internal)?;

    if !allowed {
        return Err(ApiError::NotFound("repository".into()));
    }

    // Generate presigned URLs for each object
    const EXPIRES_SECS: i64 = 3600;
    let expire_duration = Duration::from_secs(3600);
    let mut objects = Vec::with_capacity(body.objects.len());

    for obj in &body.objects {
        // A20: Validate LFS object size
        if obj.size < 0 {
            return Err(ApiError::BadRequest("invalid object size".into()));
        }
        #[allow(clippy::cast_sign_loss)]
        if (obj.size as u64) > state.config.max_lfs_object_bytes {
            return Err(ApiError::BadRequest(format!(
                "LFS object too large: {} bytes exceeds limit of {} bytes",
                obj.size, state.config.max_lfs_object_bytes
            )));
        }
        crate::validation::check_lfs_oid(&obj.oid)?;
        let path = format!("lfs/{}/{}", project.project_id, obj.oid);

        let actions = match body.operation.as_str() {
            "upload" => {
                let presigned = state.minio.presign_write(&path, expire_duration).await?;
                LfsActions {
                    upload: Some(LfsAction {
                        href: presigned.uri().to_string(),
                        header: HashMap::new(),
                        expires_in: EXPIRES_SECS,
                    }),
                    download: None,
                }
            }
            "download" => {
                let presigned = state.minio.presign_read(&path, expire_duration).await?;
                LfsActions {
                    upload: None,
                    download: Some(LfsAction {
                        href: presigned.uri().to_string(),
                        header: HashMap::new(),
                        expires_in: EXPIRES_SECS,
                    }),
                }
            }
            _ => unreachable!(), // validated above
        };

        objects.push(LfsObjectResponse {
            oid: obj.oid.clone(),
            size: obj.size,
            actions,
        });
    }

    Ok(Json(LfsBatchResponse {
        transfer: "basic".into(),
        objects,
    }))
}
