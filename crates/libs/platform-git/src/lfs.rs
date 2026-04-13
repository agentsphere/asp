// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Git LFS batch API handler.
//!
//! Provides a `router()` returning an axum `Router` generic over
//! [`GitServerServices`](crate::server_services::GitServerServices).

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::GitError;
use crate::server_services::{GitServerServices, GitServerState};
use crate::smart_http::{check_access_for_user, extract_basic_credentials};
use crate::validation::check_lfs_oid;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// LFS batch request body.
#[derive(Debug, Deserialize)]
pub struct LfsBatchRequest {
    pub operation: String,
    #[allow(dead_code)] // protocol field, validated but not used
    pub transfers: Option<Vec<String>>,
    pub objects: Vec<LfsObject>,
}

/// A single LFS object in a batch request.
#[derive(Debug, Deserialize, Serialize)]
pub struct LfsObject {
    pub oid: String,
    pub size: i64,
}

/// LFS batch response.
#[derive(Debug, Serialize)]
pub struct LfsBatchResponse {
    pub transfer: String,
    pub objects: Vec<LfsObjectResponse>,
}

/// Response for a single LFS object.
#[derive(Debug, Serialize)]
pub struct LfsObjectResponse {
    pub oid: String,
    pub size: i64,
    pub actions: LfsActions,
}

/// Upload/download actions with presigned URLs.
#[derive(Debug, Serialize)]
pub struct LfsActions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload: Option<LfsAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download: Option<LfsAction>,
}

/// A single LFS action (upload or download).
#[derive(Debug, Serialize)]
pub struct LfsAction {
    pub href: String,
    pub header: HashMap<String, String>,
    pub expires_in: i64,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Create the LFS batch API router.
pub fn router<Svc: GitServerServices>() -> Router<GitServerState<Svc>> {
    Router::new().route("/{owner}/{repo}/info/lfs/objects/batch", post(batch::<Svc>))
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `POST /:owner/:repo/info/lfs/objects/batch`
///
/// Git LFS batch API. Returns presigned URLs for upload/download.
#[tracing::instrument(skip(state, body), fields(%owner, %repo), err)]
async fn batch<Svc: GitServerServices>(
    State(state): State<GitServerState<Svc>>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<LfsBatchRequest>,
) -> Result<Json<LfsBatchResponse>, GitError> {
    // Authenticate via Basic Auth
    let (username, password) = extract_basic_credentials(&headers)?;
    let git_user = state.svc.authenticate_basic(&username, &password).await?;

    // Resolve project
    let project = state.svc.resolve(&owner, &repo).await?;

    // Enforce hard project boundary from API token
    if let Some(boundary_pid) = git_user.boundary_project_id
        && boundary_pid != project.project_id
    {
        return Err(GitError::NotFound("repository".into()));
    }

    // Enforce hard workspace boundary from API token
    if let Some(scope_wid) = git_user.boundary_workspace_id {
        let in_workspace = state
            .svc
            .check_workspace_boundary(project.project_id, scope_wid)
            .await?;
        if !in_workspace {
            return Err(GitError::NotFound("repository".into()));
        }
    }

    // Check permission: download = read, upload = write
    let is_read = match body.operation.as_str() {
        "download" => true,
        "upload" => false,
        _ => return Err(GitError::BadRequest("invalid operation".into())),
    };

    check_access_for_user(&state.svc, &git_user, &project, is_read).await?;

    // Generate presigned URLs for each object
    const EXPIRES_SECS: i64 = 3600;
    let expire_duration = Duration::from_secs(3600);
    let max_size = state.svc.max_lfs_object_bytes();
    let mut objects = Vec::with_capacity(body.objects.len());

    for obj in &body.objects {
        if obj.size < 0 {
            return Err(GitError::BadRequest("invalid object size".into()));
        }
        #[allow(clippy::cast_sign_loss)]
        if (obj.size as u64) > max_size {
            return Err(GitError::BadRequest(format!(
                "LFS object too large: {} bytes exceeds limit of {} bytes",
                obj.size, max_size
            )));
        }
        check_lfs_oid(&obj.oid)?;
        let path = format!("lfs/{}/{}", project.project_id, obj.oid);

        let actions = match body.operation.as_str() {
            "upload" => {
                let presigned = state
                    .svc
                    .presign_lfs_write(&path, expire_duration)
                    .await
                    .map_err(GitError::Other)?;
                LfsActions {
                    upload: Some(LfsAction {
                        href: presigned,
                        header: HashMap::new(),
                        expires_in: EXPIRES_SECS,
                    }),
                    download: None,
                }
            }
            "download" => {
                let presigned = state
                    .svc
                    .presign_lfs_read(&path, expire_duration)
                    .await
                    .map_err(GitError::Other)?;
                LfsActions {
                    upload: None,
                    download: Some(LfsAction {
                        href: presigned,
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfs_batch_request_deserialization() {
        let json = serde_json::json!({
            "operation": "download",
            "objects": [
                {"oid": "abc123", "size": 1234}
            ]
        });
        let req: LfsBatchRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.operation, "download");
        assert_eq!(req.objects.len(), 1);
        assert_eq!(req.objects[0].oid, "abc123");
        assert!(req.transfers.is_none());
    }

    #[test]
    fn lfs_batch_response_serialization() {
        let resp = LfsBatchResponse {
            transfer: "basic".into(),
            objects: vec![LfsObjectResponse {
                oid: "abc123".into(),
                size: 1234,
                actions: LfsActions {
                    upload: None,
                    download: Some(LfsAction {
                        href: "https://example.com/download".into(),
                        header: HashMap::new(),
                        expires_in: 3600,
                    }),
                },
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["transfer"], "basic");
        assert_eq!(json["objects"][0]["oid"], "abc123");
        assert!(json["objects"][0]["actions"].get("upload").is_none());
        assert!(
            json["objects"][0]["actions"]["download"]["href"]
                .as_str()
                .unwrap()
                .starts_with("https://")
        );
    }

    #[test]
    fn lfs_actions_skip_none() {
        let actions = LfsActions {
            upload: None,
            download: None,
        };
        let json = serde_json::to_value(&actions).unwrap();
        assert!(json.get("upload").is_none());
        assert!(json.get("download").is_none());
    }

    #[test]
    fn lfs_object_roundtrip() {
        let obj = LfsObject {
            oid: "deadbeef".into(),
            size: 42,
        };
        let json = serde_json::to_string(&obj).unwrap();
        let parsed: LfsObject = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.oid, "deadbeef");
        assert_eq!(parsed.size, 42);
    }
}
