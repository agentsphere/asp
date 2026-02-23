use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use super::auth::RegistryUser;
use super::error::RegistryError;
use super::types::TagListResponse;
use super::{RepoAccess, resolve_repo_with_access};
use crate::store::AppState;

#[derive(Debug, Deserialize)]
pub struct TagListQuery {
    /// Maximum number of tags to return.
    pub n: Option<i64>,
    /// Cursor: return tags lexicographically after this value.
    pub last: Option<String>,
}

// ---------------------------------------------------------------------------
// GET /v2/{name}/tags/list
// ---------------------------------------------------------------------------

pub async fn list_tags(
    State(state): State<AppState>,
    user: RegistryUser,
    Path(name): Path<String>,
    Query(query): Query<TagListQuery>,
) -> Result<Json<TagListResponse>, RegistryError> {
    let RepoAccess {
        repository_id,
        project_id: _,
    } = resolve_repo_with_access(&state, &user, &name, false).await?;

    let limit = query.n.unwrap_or(100).min(1000);

    let tags = if let Some(ref last) = query.last {
        sqlx::query_scalar!(
            r#"SELECT name FROM registry_tags
               WHERE repository_id = $1 AND name > $2
               ORDER BY name
               LIMIT $3"#,
            repository_id,
            last,
            limit,
        )
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_scalar!(
            r#"SELECT name FROM registry_tags
               WHERE repository_id = $1
               ORDER BY name
               LIMIT $2"#,
            repository_id,
            limit,
        )
        .fetch_all(&state.pool)
        .await?
    };

    Ok(Json(TagListResponse { name, tags }))
}
