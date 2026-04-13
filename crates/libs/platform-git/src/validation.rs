// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Git-related validation functions.

use crate::error::GitError;

/// Validate a git ref (branch name, tag, SHA) for shell safety.
///
/// Rejects empty strings and strings containing dangerous characters:
/// `..`, `;`, `|`, `$`, `` ` ``, `\n`, `\0`, ` `.
pub fn validate_git_ref(git_ref: &str) -> Result<(), GitError> {
    if git_ref.is_empty()
        || git_ref.contains("..")
        || git_ref.contains(';')
        || git_ref.contains('|')
        || git_ref.contains('$')
        || git_ref.contains('`')
        || git_ref.contains('\n')
        || git_ref.contains('\0')
        || git_ref.contains(' ')
    {
        return Err(GitError::InvalidRef(git_ref.to_string()));
    }
    Ok(())
}

/// Validate a file path for directory traversal.
///
/// Rejects paths containing `..` or null bytes.
pub fn validate_path(path: &str) -> Result<(), GitError> {
    if path.contains("..") || path.contains('\0') {
        return Err(GitError::PathTraversal(path.to_string()));
    }
    Ok(())
}

/// Validate a tag name. Returns `false` for empty or dangerous names.
pub fn validate_tag_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.contains('\0')
        && !name.contains(';')
        && !name.contains('|')
        && !name.contains('$')
        && !name.contains('`')
}

/// Match a glob pattern against a value.
///
/// Supports `*` as a wildcard (matches any sequence of characters).
/// Without `*`, performs exact match.
pub fn match_glob_pattern(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if !pattern.contains('*') {
        return pattern == value;
    }

    let segments: Vec<&str> = pattern.split('*').collect();

    // First segment must be a prefix of value
    let prefix = segments[0];
    if !value.starts_with(prefix) {
        return false;
    }

    // Last segment must be a suffix of the remaining string
    let suffix = segments[segments.len() - 1];
    // Check that there's enough room for prefix + suffix (handles overlap edge case)
    if value.len() < prefix.len() + suffix.len() {
        return false;
    }
    if !value.ends_with(suffix) {
        return false;
    }

    // Walk middle segments in order, each must be found after the previous match
    let mut cursor = prefix.len();
    let end = value.len() - suffix.len();
    for &seg in &segments[1..segments.len() - 1] {
        if let Some(pos) = value[cursor..end].find(seg) {
            cursor += pos + seg.len();
        } else {
            return false;
        }
    }

    true
}

/// Validate a Git LFS OID (SHA-256 hash).
///
/// Must be exactly 64 hexadecimal characters.
pub fn check_lfs_oid(oid: &str) -> Result<(), GitError> {
    if oid.len() != 64 || !oid.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(GitError::BadRequest(
            "invalid LFS OID: must be 64 hex characters (SHA-256)".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- validate_git_ref --

    #[test]
    fn validate_ref_accepts_valid() {
        assert!(validate_git_ref("main").is_ok());
        assert!(validate_git_ref("feature/foo").is_ok());
        assert!(validate_git_ref("v1.0.0").is_ok());
        assert!(validate_git_ref("HEAD").is_ok());
        assert!(validate_git_ref("v1.0.0-rc.1+build.123").is_ok());
    }

    #[test]
    fn validate_ref_rejects_invalid() {
        assert!(validate_git_ref("").is_err());
        assert!(validate_git_ref("foo..bar").is_err());
        assert!(validate_git_ref("foo;rm").is_err());
        assert!(validate_git_ref("foo|bar").is_err());
        assert!(validate_git_ref("$HOME").is_err());
        assert!(validate_git_ref("`cmd`").is_err());
        assert!(validate_git_ref("foo\nbar").is_err());
        assert!(validate_git_ref("foo\0bar").is_err());
        assert!(validate_git_ref("foo bar").is_err());
    }

    // -- validate_path --

    #[test]
    fn validate_path_accepts_valid() {
        assert!(validate_path("src/main.rs").is_ok());
        assert!(validate_path("/").is_ok());
        assert!(validate_path("").is_ok());
        assert!(validate_path("a/b/c/d/e/f/g/h.txt").is_ok());
    }

    #[test]
    fn validate_path_rejects_traversal() {
        assert!(validate_path("../etc/passwd").is_err());
        assert!(validate_path("foo/../../bar").is_err());
        assert!(validate_path("src/\0main.rs").is_err());
    }

    // -- validate_tag_name --

    #[test]
    fn validate_tag_name_valid() {
        assert!(validate_tag_name("v1.0.0"));
        assert!(validate_tag_name("release/v2.0"));
        assert!(validate_tag_name("v1.0.0-beta+build.123"));
    }

    #[test]
    fn validate_tag_name_invalid() {
        assert!(!validate_tag_name(""));
        assert!(!validate_tag_name("v1..0"));
        assert!(!validate_tag_name("v1;evil"));
        assert!(!validate_tag_name("v1|cat"));
        assert!(!validate_tag_name("$HOME"));
        assert!(!validate_tag_name("v1`cmd`"));
    }

    // -- match_glob_pattern --

    #[test]
    fn glob_exact_match() {
        assert!(match_glob_pattern("main", "main"));
        assert!(!match_glob_pattern("main", "develop"));
    }

    #[test]
    fn glob_wildcard_all() {
        assert!(match_glob_pattern("*", "anything"));
        assert!(match_glob_pattern("*", ""));
    }

    #[test]
    fn glob_prefix_wildcard() {
        assert!(match_glob_pattern("release/*", "release/v1.0"));
        assert!(match_glob_pattern("release/*", "release/"));
        assert!(!match_glob_pattern("release/*", "feature/v1.0"));
    }

    #[test]
    fn glob_suffix_wildcard() {
        assert!(match_glob_pattern("*-stable", "v1-stable"));
        assert!(!match_glob_pattern("*-stable", "v1-beta"));
    }

    #[test]
    fn glob_middle_wildcard() {
        assert!(match_glob_pattern("feat*bar", "feat-foobar"));
        assert!(match_glob_pattern("feat*bar", "featbar"));
        assert!(!match_glob_pattern("feat*bar", "feat-foobaz"));
    }

    #[test]
    fn glob_multiple_wildcards() {
        assert!(match_glob_pattern("a*b*c", "axbyc"));
        assert!(match_glob_pattern("a*b*c", "abc"));
        assert!(!match_glob_pattern("a*b*c", "axyz"));
    }

    #[test]
    fn glob_overlap_edge_case() {
        // Pattern "ab*ab" should match "abab" but not "ab"
        assert!(match_glob_pattern("ab*ab", "abab"));
        assert!(!match_glob_pattern("ab*ab", "ab"));
    }

    // -- check_lfs_oid --

    #[test]
    fn lfs_oid_valid() {
        let oid = "a".repeat(64);
        assert!(check_lfs_oid(&oid).is_ok());
        let oid = "0123456789abcdef".repeat(4);
        assert!(check_lfs_oid(&oid).is_ok());
        let oid = "0123456789ABCDEF".repeat(4);
        assert!(check_lfs_oid(&oid).is_ok());
    }

    #[test]
    fn lfs_oid_too_short() {
        let oid = "a".repeat(63);
        assert!(check_lfs_oid(&oid).is_err());
    }

    #[test]
    fn lfs_oid_too_long() {
        let oid = "a".repeat(65);
        assert!(check_lfs_oid(&oid).is_err());
    }

    #[test]
    fn lfs_oid_non_hex() {
        let mut oid = "a".repeat(63);
        oid.push('g');
        assert!(check_lfs_oid(&oid).is_err());
    }

    #[test]
    fn lfs_oid_empty() {
        assert!(check_lfs_oid("").is_err());
    }
}
