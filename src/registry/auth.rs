use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use uuid::Uuid;

use crate::auth::token;
use crate::store::AppState;

/// Authenticated registry user extracted from the request.
/// Supports both Bearer token and Basic auth (for `docker login`).
#[derive(Debug, Clone)]
#[allow(dead_code)] // user_name used for audit logging
pub struct RegistryUser {
    pub user_id: Uuid,
    pub user_name: String,
    /// Hard project boundary from API token.
    pub boundary_project_id: Option<Uuid>,
    /// Hard workspace boundary from API token.
    pub boundary_workspace_id: Option<Uuid>,
    /// When non-NULL, limits which image name:tag this token can push to (glob pattern).
    pub registry_tag_pattern: Option<String>,
    /// Token permission scopes (None = password auth, Some = API token auth).
    pub token_scopes: Option<Vec<String>>,
}

/// Rejection type for registry auth — returns OCI-compliant 401 with Www-Authenticate.
pub struct RegistryAuthRejection;

impl IntoResponse for RegistryAuthRejection {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "errors": [{
                "code": "UNAUTHORIZED",
                "message": "authentication required",
                "detail": {}
            }]
        });

        let mut headers = HeaderMap::new();
        headers.insert(
            "www-authenticate",
            HeaderValue::from_static(r#"Basic realm="platform-registry""#),
        );

        (StatusCode::UNAUTHORIZED, headers, axum::Json(body)).into_response()
    }
}

/// Rejection for rate-limited registry requests — returns 429 with Retry-After.
pub struct RegistryRateLimitRejection;

impl IntoResponse for RegistryRateLimitRejection {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "errors": [{
                "code": "TOOMANYREQUESTS",
                "message": "too many requests",
                "detail": {}
            }]
        });

        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("30"));

        (StatusCode::TOO_MANY_REQUESTS, headers, axum::Json(body)).into_response()
    }
}

struct TokenLookup {
    user_id: Uuid,
    user_name: String,
    is_active: bool,
    scope_project_id: Option<Uuid>,
    scope_workspace_id: Option<Uuid>,
    registry_tag_pattern: Option<String>,
    scopes: Vec<String>,
}

impl FromRequestParts<AppState> for RegistryUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok());

        let Some(auth_value) = auth_header else {
            return Err(RegistryAuthRejection.into_response());
        };

        // Try Bearer token first
        if let Some(raw_token) = auth_value.strip_prefix("Bearer ") {
            if !raw_token.is_empty()
                && let Some(user) = lookup_api_token(&state.pool, raw_token).await
                && user.is_active
            {
                return Ok(Self {
                    user_id: user.user_id,
                    user_name: user.user_name,
                    boundary_project_id: user.scope_project_id,
                    boundary_workspace_id: user.scope_workspace_id,
                    registry_tag_pattern: user.registry_tag_pattern,
                    token_scopes: Some(user.scopes), // A8: enforce token scopes
                });
            }
            return Err(RegistryAuthRejection.into_response());
        }

        // Try Basic auth (docker login sends user:password as base64)
        if let Some(encoded) = auth_value.strip_prefix("Basic ") {
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded)
                && let Ok(creds) = String::from_utf8(decoded)
                && let Some((username, password)) = creds.split_once(':')
            {
                // S53: rate-limit registry basic auth.
                // 4 pipelines × 3 steps × ~50 auth calls/step = ~600 calls per 5min window.
                if crate::auth::rate_limit::check_rate(
                    &state.valkey,
                    "registry_auth",
                    username,
                    2000,
                    300,
                )
                .await
                .is_err()
                {
                    return Err(RegistryRateLimitRejection.into_response());
                }
                if let Some(user) = lookup_basic_auth(&state.pool, username, password).await
                    && user.is_active
                {
                    return Ok(Self {
                        user_id: user.user_id,
                        user_name: user.user_name,
                        boundary_project_id: user.scope_project_id,
                        boundary_workspace_id: user.scope_workspace_id,
                        registry_tag_pattern: user.registry_tag_pattern,
                        token_scopes: Some(user.scopes), // A8: enforce token scopes
                    });
                }
            }
            return Err(RegistryAuthRejection.into_response());
        }

        Err(RegistryAuthRejection.into_response())
    }
}

/// Look up an API token by its raw value (same logic as `auth::middleware`).
async fn lookup_api_token(pool: &sqlx::PgPool, raw_token: &str) -> Option<TokenLookup> {
    let hash = token::hash_token(raw_token);

    let row = sqlx::query_as!(
        TokenLookup,
        r#"
        SELECT u.id as "user_id!", u.name as "user_name!", u.is_active as "is_active!",
               t.project_id as "scope_project_id?", t.scope_workspace_id,
               t.registry_tag_pattern, t.scopes
        FROM api_tokens t
        JOIN users u ON u.id = t.user_id
        WHERE t.token_hash = $1
          AND (t.expires_at IS NULL OR t.expires_at > now())
        "#,
        hash,
    )
    .fetch_optional(pool)
    .await
    .ok()?;

    if row.is_some() {
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
    }

    row
}

/// Basic auth: username is the user name, password is an API token.
async fn lookup_basic_auth(
    pool: &sqlx::PgPool,
    username: &str,
    password: &str,
) -> Option<TokenLookup> {
    // The password is the raw API token — look it up
    let hash = token::hash_token(password);

    let row = sqlx::query_as!(
        TokenLookup,
        r#"
        SELECT u.id as "user_id!", u.name as "user_name!", u.is_active as "is_active!",
               t.project_id as "scope_project_id?", t.scope_workspace_id,
               t.registry_tag_pattern, t.scopes
        FROM api_tokens t
        JOIN users u ON u.id = t.user_id
        WHERE t.token_hash = $1
          AND u.name = $2
          AND (t.expires_at IS NULL OR t.expires_at > now())
        "#,
        hash,
        username,
    )
    .fetch_optional(pool)
    .await
    .ok()?;

    if row.is_some() {
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
    }

    row
}

/// Optional registry auth — returns `None` instead of 401 when credentials are missing.
/// Used on pull/read endpoints to allow anonymous access to public projects.
pub struct OptionalRegistryUser(pub Option<RegistryUser>);

impl FromRequestParts<AppState> for OptionalRegistryUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match RegistryUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(Self(Some(user))),
            Err(_) => Ok(Self(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_auth_rejection_is_401() {
        let response = RegistryAuthRejection.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key("www-authenticate"));
    }

    #[tokio::test]
    async fn registry_auth_rejection_body_has_unauthorized_code() {
        let response = RegistryAuthRejection.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let www_auth = response
            .headers()
            .get("www-authenticate")
            .expect("should have www-authenticate header");
        assert_eq!(
            www_auth.to_str().unwrap(),
            r#"Basic realm="platform-registry""#
        );
        let bytes = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["errors"].is_array());
        assert_eq!(json["errors"][0]["code"], "UNAUTHORIZED");
        assert_eq!(json["errors"][0]["message"], "authentication required");
    }

    #[test]
    fn registry_rate_limit_rejection_is_429() {
        let response = RegistryRateLimitRejection.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().contains_key("retry-after"));
        assert_eq!(
            response
                .headers()
                .get("retry-after")
                .unwrap()
                .to_str()
                .unwrap(),
            "30"
        );
    }

    #[tokio::test]
    async fn registry_rate_limit_rejection_body_has_toomanyrequests_code() {
        let response = RegistryRateLimitRejection.into_response();
        let bytes = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["errors"].is_array());
        assert_eq!(json["errors"][0]["code"], "TOOMANYREQUESTS");
        assert_eq!(json["errors"][0]["message"], "too many requests");
    }

    #[test]
    fn registry_user_fields() {
        let user = RegistryUser {
            user_id: Uuid::nil(),
            user_name: "alice".into(),
            boundary_project_id: None,
            boundary_workspace_id: None,
            registry_tag_pattern: Some("myapp:*".into()),
            token_scopes: Some(vec!["registry:push".into()]),
        };
        assert_eq!(user.user_name, "alice");
        assert_eq!(user.registry_tag_pattern.as_deref(), Some("myapp:*"));
        assert!(user.boundary_project_id.is_none());
    }

    #[test]
    fn registry_user_debug() {
        let user = RegistryUser {
            user_id: Uuid::nil(),
            user_name: "bob".into(),
            boundary_project_id: None,
            boundary_workspace_id: None,
            registry_tag_pattern: None,
            token_scopes: None,
        };
        let debug = format!("{user:?}");
        assert!(debug.contains("bob"));
    }

    #[test]
    fn registry_user_clone() {
        let user = RegistryUser {
            user_id: Uuid::nil(),
            user_name: "charlie".into(),
            boundary_project_id: Some(Uuid::nil()),
            boundary_workspace_id: Some(Uuid::nil()),
            registry_tag_pattern: Some("proj:*".into()),
            token_scopes: Some(vec!["registry:pull".into()]),
        };
        let cloned = user.clone();
        assert_eq!(cloned.user_id, user.user_id);
        assert_eq!(cloned.user_name, user.user_name);
        assert_eq!(cloned.registry_tag_pattern, user.registry_tag_pattern);
        assert_eq!(cloned.token_scopes, user.token_scopes);
    }

    #[test]
    fn registry_user_no_scopes() {
        let user = RegistryUser {
            user_id: Uuid::nil(),
            user_name: "noauth".into(),
            boundary_project_id: None,
            boundary_workspace_id: None,
            registry_tag_pattern: None,
            token_scopes: None,
        };
        assert!(user.token_scopes.is_none());
        assert!(user.registry_tag_pattern.is_none());
    }

    #[tokio::test]
    async fn registry_auth_rejection_json_errors_array() {
        let response = RegistryAuthRejection.into_response();
        let bytes = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // Verify the structure matches OCI spec
        assert!(json["errors"].is_array());
        assert_eq!(json["errors"].as_array().unwrap().len(), 1);
        assert!(json["errors"][0]["detail"].is_object());
    }

    #[tokio::test]
    async fn registry_rate_limit_rejection_json_structure() {
        let response = RegistryRateLimitRejection.into_response();
        let bytes = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["errors"].is_array());
        assert!(json["errors"][0]["detail"].is_object());
    }

    #[test]
    fn registry_user_with_all_boundaries() {
        let pid = Uuid::new_v4();
        let wid = Uuid::new_v4();
        let user = RegistryUser {
            user_id: Uuid::new_v4(),
            user_name: "scoped".into(),
            boundary_project_id: Some(pid),
            boundary_workspace_id: Some(wid),
            registry_tag_pattern: Some("myapp-dev:session-*".into()),
            token_scopes: Some(vec!["registry:push".into(), "registry:pull".into()]),
        };
        assert_eq!(user.boundary_project_id, Some(pid));
        assert_eq!(user.boundary_workspace_id, Some(wid));
        assert_eq!(user.token_scopes.as_ref().unwrap().len(), 2);
    }
}
