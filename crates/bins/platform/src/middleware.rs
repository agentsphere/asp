// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Auth middleware — `FromRequestParts<PlatformState>` impl for `AuthUser`.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use platform_auth::{extract_bearer_token, extract_ip, extract_session_cookie};
use platform_auth::{hash_token, lookup_api_token, lookup_session};
use platform_types::{ApiError, AuthUser, UserType, parse_user_type};
use uuid::Uuid;

use crate::state::PlatformState;

impl FromRequestParts<PlatformState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &PlatformState,
    ) -> Result<Self, Self::Rejection> {
        let trust_proxy = state.config.auth.trust_proxy_headers;
        let ip_addr = extract_ip(parts, trust_proxy, &state.config.auth.trust_proxy_cidrs);

        // Try Bearer token — check API tokens first, then session tokens
        if let Some(raw_token) = extract_bearer_token(parts) {
            if let Some(user) = lookup_api_token(&state.pool, raw_token).await? {
                if !user.is_active {
                    return Err(ApiError::Unauthorized);
                }
                let user_type = parse_user_type(&user.user_type)?;
                let session_id = if user_type == UserType::Agent {
                    user.name
                        .strip_prefix("agent-session-")
                        .and_then(|s| Uuid::parse_str(s).ok())
                } else {
                    None
                };
                let auth_user = Self {
                    user_id: user.user_id,
                    user_name: user.user_name,
                    user_type,
                    ip_addr,
                    token_scopes: Some(user.scopes),
                    boundary_workspace_id: user.scope_workspace_id,
                    boundary_project_id: user.scope_project_id,
                    session_id,
                    session_token_hash: None,
                };
                auth_user.record_to_span();
                return Ok(auth_user);
            }
            // Bearer token not in api_tokens — try as session token
            if let Some(user) = lookup_session(&state.pool, raw_token).await? {
                if !user.is_active {
                    return Err(ApiError::Unauthorized);
                }
                let user_type = parse_user_type(&user.user_type)?;
                if !user_type.can_login() {
                    return Err(ApiError::Unauthorized);
                }
                let auth_user = Self {
                    user_id: user.user_id,
                    user_name: user.user_name,
                    user_type,
                    ip_addr,
                    token_scopes: None,
                    boundary_workspace_id: None,
                    boundary_project_id: None,
                    session_id: None,
                    session_token_hash: Some(hash_token(raw_token)),
                };
                auth_user.record_to_span();
                return Ok(auth_user);
            }
        }

        // Try session cookie
        if let Some(session_token) = extract_session_cookie(parts)
            && let Some(user) = lookup_session(&state.pool, session_token).await?
        {
            if !user.is_active {
                return Err(ApiError::Unauthorized);
            }
            let user_type = parse_user_type(&user.user_type)?;
            if !user_type.can_login() {
                return Err(ApiError::Unauthorized);
            }
            let auth_user = Self {
                user_id: user.user_id,
                user_name: user.user_name,
                user_type,
                ip_addr,
                token_scopes: None,
                boundary_workspace_id: None,
                boundary_project_id: None,
                session_id: None,
                session_token_hash: Some(hash_token(session_token)),
            };
            auth_user.record_to_span();
            return Ok(auth_user);
        }

        Err(ApiError::Unauthorized)
    }
}
