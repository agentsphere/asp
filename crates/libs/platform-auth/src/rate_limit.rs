// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use fred::interfaces::KeysInterface;
use fred::types::ExpireOptions;

use platform_types::ApiError;

/// Fixed-window rate limiter backed by Valkey.
pub async fn check_rate(
    valkey: &fred::clients::Pool,
    prefix: &str,
    identifier: &str,
    max_attempts: u64,
    window_secs: i64,
) -> Result<(), ApiError> {
    let key = format!("rate:{prefix}:{identifier}");

    let count: u64 = valkey.incr(&key).await.map_err(ApiError::from)?;

    let _: () = valkey
        .expire(&key, window_secs, Some(ExpireOptions::NX))
        .await
        .map_err(ApiError::from)?;

    check_rate_result(count, max_attempts)
}

/// Pure threshold check.
fn check_rate_result(count: u64, max_attempts: u64) -> Result<(), ApiError> {
    if count > max_attempts {
        return Err(ApiError::TooManyRequests);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_first_request_allowed() {
        assert!(check_rate_result(1, 10).is_ok());
    }

    #[test]
    fn rate_limit_at_threshold_allowed() {
        assert!(check_rate_result(10, 10).is_ok());
    }

    #[test]
    fn rate_limit_over_threshold_rejected() {
        assert!(check_rate_result(11, 10).is_err());
    }

    #[test]
    fn rate_limit_error_type_is_too_many_requests() {
        let err = check_rate_result(11, 10).unwrap_err();
        assert!(
            matches!(err, ApiError::TooManyRequests),
            "expected TooManyRequests, got: {err:?}"
        );
    }

    #[test]
    fn rate_limit_zero_max_rejects_immediately() {
        assert!(check_rate_result(1, 0).is_err());
    }

    #[test]
    fn rate_limit_boundary_values() {
        assert!(check_rate_result(u64::MAX, u64::MAX).is_ok());
        assert!(check_rate_result(0, 0).is_ok());
    }
}
