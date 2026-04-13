// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use crate::error::ApiError;

/// Check that a string field length is within [min, max].
pub fn check_length(field: &str, value: &str, min: usize, max: usize) -> Result<(), ApiError> {
    let len = value.len();
    if len < min || len > max {
        return Err(ApiError::BadRequest(format!(
            "{field} must be between {min} and {max} characters (got {len})"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_length_ok() {
        assert!(check_length("name", "hello", 1, 10).is_ok());
    }

    #[test]
    fn check_length_too_short() {
        assert!(check_length("name", "", 1, 10).is_err());
    }

    #[test]
    fn check_length_too_long() {
        assert!(check_length("name", "hello world!", 1, 5).is_err());
    }

    #[test]
    fn check_length_exact_boundary() {
        assert!(check_length("name", "12345", 5, 5).is_ok());
    }
}
