// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! SSH git command parsing.

use crate::error::SshError;

// ---------------------------------------------------------------------------
// Command parsing
// ---------------------------------------------------------------------------

/// Parsed SSH git command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    pub owner: String,
    pub repo: String,
    pub is_read: bool,
}

/// Parse an SSH exec command like `git-upload-pack 'owner/repo.git'`.
///
/// Returns the owner, repo name (without `.git` suffix), and whether this is a
/// read operation (`git-upload-pack`) vs write (`git-receive-pack`).
pub fn parse_ssh_command(command: &str) -> Result<ParsedCommand, SshError> {
    let command = command.trim();
    if command.is_empty() {
        return Err(SshError::InvalidCommand);
    }

    let (service, path) = command.split_once(' ').ok_or(SshError::InvalidCommand)?;

    let is_read = match service {
        "git-upload-pack" => true,
        "git-receive-pack" => false,
        _ => return Err(SshError::UnsupportedService(service.to_string())),
    };

    // Strip surrounding quotes (single or double)
    let path = path.trim();
    let path = strip_quotes(path);

    // Strip leading /
    let path = path.strip_prefix('/').unwrap_or(path);

    // Reject dangerous characters
    if path.contains("..")
        || path.contains('\0')
        || path.contains('\n')
        || path.contains(';')
        || path.contains('|')
        || path.contains('`')
        || path.contains('$')
        || path.contains(' ')
    {
        return Err(SshError::PathTraversal);
    }

    // Strip .git suffix
    let path = path.strip_suffix(".git").unwrap_or(path);

    // Split into owner/repo — must be exactly two segments
    let (owner, repo) = path.split_once('/').ok_or(SshError::InvalidCommand)?;

    if repo.contains('/') || owner.is_empty() || repo.is_empty() {
        return Err(SshError::InvalidCommand);
    }

    Ok(ParsedCommand {
        owner: owner.to_string(),
        repo: repo.to_string(),
        is_read,
    })
}

fn strip_quotes(s: &str) -> &str {
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // parse_ssh_command — happy paths
    // -----------------------------------------------------------------------

    #[test]
    fn parse_upload_pack_with_git_suffix() {
        let result = parse_ssh_command("git-upload-pack 'owner/repo.git'").unwrap();
        assert_eq!(result.owner, "owner");
        assert_eq!(result.repo, "repo");
        assert!(result.is_read);
    }

    #[test]
    fn parse_receive_pack_with_git_suffix() {
        let cmd = parse_ssh_command("git-receive-pack 'myorg/myapp.git'").unwrap();
        assert_eq!(cmd.owner, "myorg");
        assert_eq!(cmd.repo, "myapp");
        assert!(!cmd.is_read);
    }

    #[test]
    fn parse_upload_pack_no_git_suffix() {
        let cmd = parse_ssh_command("git-upload-pack 'alice/tools'").unwrap();
        assert_eq!(cmd.owner, "alice");
        assert_eq!(cmd.repo, "tools");
        assert!(cmd.is_read);
    }

    #[test]
    fn parse_receive_pack_no_git_suffix() {
        let result = parse_ssh_command("git-receive-pack 'owner/repo'").unwrap();
        assert_eq!(result.owner, "owner");
        assert_eq!(result.repo, "repo");
        assert!(!result.is_read);
    }

    #[test]
    fn parse_double_quoted_path() {
        let result = parse_ssh_command("git-upload-pack \"owner/repo.git\"").unwrap();
        assert_eq!(result.owner, "owner");
        assert_eq!(result.repo, "repo");
    }

    #[test]
    fn parse_leading_slash_stripped() {
        let result = parse_ssh_command("git-upload-pack '/owner/repo.git'").unwrap();
        assert_eq!(result.owner, "owner");
        assert_eq!(result.repo, "repo");
    }

    #[test]
    fn parse_whitespace_trimmed() {
        let result = parse_ssh_command("  git-upload-pack 'owner/repo.git'  ").unwrap();
        assert_eq!(result.owner, "owner");
        assert_eq!(result.repo, "repo");
        assert!(result.is_read);
    }

    #[test]
    fn parse_single_char_owner_and_repo() {
        let result = parse_ssh_command("git-upload-pack 'a/b.git'").unwrap();
        assert_eq!(result.owner, "a");
        assert_eq!(result.repo, "b");
    }

    #[test]
    fn parse_long_owner_and_repo() {
        let long_name = "a".repeat(200);
        let cmd = format!("git-upload-pack '{long_name}/{long_name}.git'");
        let result = parse_ssh_command(&cmd).unwrap();
        assert_eq!(result.owner, long_name);
        assert_eq!(result.repo, long_name);
    }

    #[test]
    fn parse_hyphen_underscore_dot_in_names() {
        let result =
            parse_ssh_command("git-upload-pack 'my-org_name/my.repo-name_v2.git'").unwrap();
        assert_eq!(result.owner, "my-org_name");
        assert_eq!(result.repo, "my.repo-name_v2");
    }

    #[test]
    fn parse_double_git_suffix() {
        let result = parse_ssh_command("git-upload-pack 'owner/repo.git.git'").unwrap();
        assert_eq!(result.owner, "owner");
        assert_eq!(result.repo, "repo.git");
    }

    #[test]
    fn parse_unicode_in_path() {
        let result = parse_ssh_command("git-upload-pack 'owner/repo-名前'");
        assert!(result.is_ok(), "unicode should be allowed: {result:?}");
        let parsed = result.unwrap();
        assert_eq!(parsed.repo, "repo-名前");
    }

    #[test]
    fn parse_repo_name_with_dot() {
        let result = parse_ssh_command("git-upload-pack 'owner/my.app'").unwrap();
        assert_eq!(result.repo, "my.app");
    }

    #[test]
    fn parse_repo_with_numbers() {
        let result = parse_ssh_command("git-upload-pack 'org123/repo456.git'").unwrap();
        assert_eq!(result.owner, "org123");
        assert_eq!(result.repo, "repo456");
    }

    // -----------------------------------------------------------------------
    // parse_ssh_command — rejection tests
    // -----------------------------------------------------------------------

    #[test]
    fn reject_empty_string() {
        assert!(matches!(
            parse_ssh_command(""),
            Err(SshError::InvalidCommand)
        ));
    }

    #[test]
    fn reject_only_whitespace() {
        assert!(matches!(
            parse_ssh_command("   "),
            Err(SshError::InvalidCommand)
        ));
    }

    #[test]
    fn reject_unsupported_service() {
        let result = parse_ssh_command("git-diff 'owner/repo'");
        assert!(matches!(result, Err(SshError::UnsupportedService(ref s)) if s == "git-diff"),);
    }

    #[test]
    fn reject_no_space_before_path() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack'owner/repo'"),
            Err(SshError::InvalidCommand)
        ));
    }

    #[test]
    fn reject_tab_separator() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack\towner/repo"),
            Err(SshError::InvalidCommand)
        ));
    }

    #[test]
    fn reject_no_slash_in_path() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack 'justreponame'"),
            Err(SshError::InvalidCommand)
        ));
    }

    #[test]
    fn reject_empty_owner() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack '/repo.git'"),
            Err(SshError::InvalidCommand)
        ));
    }

    #[test]
    fn reject_empty_repo() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack 'owner/.git'"),
            Err(SshError::InvalidCommand)
        ));
    }

    #[test]
    fn reject_double_dot_in_repo() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack 'owner/repo..name'"),
            Err(SshError::PathTraversal)
        ));
    }

    #[test]
    fn reject_pipe_injection() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack 'owner/repo|evil'"),
            Err(SshError::PathTraversal)
        ));
    }

    #[test]
    fn reject_backtick_injection() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack 'owner/repo`id`'"),
            Err(SshError::PathTraversal)
        ));
    }

    #[test]
    fn reject_dollar_injection() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack 'owner/$HOME'"),
            Err(SshError::PathTraversal)
        ));
    }

    #[test]
    fn reject_space_in_path() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack 'owner/repo name'"),
            Err(SshError::PathTraversal)
        ));
    }

    #[test]
    fn reject_newline_injection() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack 'owner/repo\nmalicious'"),
            Err(SshError::PathTraversal)
        ));
    }

    #[test]
    fn reject_extra_args() {
        assert!(matches!(
            parse_ssh_command("git-upload-pack owner/repo extra"),
            Err(SshError::PathTraversal)
        ));
    }

    // -----------------------------------------------------------------------
    // strip_quotes tests
    // -----------------------------------------------------------------------

    #[test]
    fn strip_quotes_single_quotes() {
        assert_eq!(strip_quotes("'hello'"), "hello");
    }

    #[test]
    fn strip_quotes_double_quotes() {
        assert_eq!(strip_quotes("\"hello\""), "hello");
    }

    #[test]
    fn strip_quotes_no_quotes() {
        assert_eq!(strip_quotes("hello"), "hello");
    }

    #[test]
    fn strip_quotes_mismatched_quotes() {
        assert_eq!(strip_quotes("'hello\""), "'hello\"");
    }

    #[test]
    fn strip_quotes_empty_string() {
        assert_eq!(strip_quotes(""), "");
    }

    #[test]
    fn strip_quotes_single_char_quoted() {
        assert_eq!(strip_quotes("'x'"), "x");
    }

    #[test]
    fn strip_quotes_empty_quoted_string() {
        assert_eq!(strip_quotes("''"), "");
    }

    // -----------------------------------------------------------------------
    // SshError Display
    // -----------------------------------------------------------------------

    #[test]
    fn ssh_error_display() {
        assert_eq!(SshError::InvalidCommand.to_string(), "invalid command");
        assert_eq!(
            SshError::PathTraversal.to_string(),
            "dangerous path rejected"
        );
        assert_eq!(
            SshError::UnsupportedService("git-archive".into()).to_string(),
            "unsupported service: git-archive"
        );
    }

    // -----------------------------------------------------------------------
    // ParsedCommand traits
    // -----------------------------------------------------------------------

    #[test]
    fn parsed_command_equality() {
        let cmd1 = parse_ssh_command("git-upload-pack 'owner/repo.git'").unwrap();
        let cmd2 = parse_ssh_command("git-upload-pack 'owner/repo.git'").unwrap();
        assert_eq!(cmd1, cmd2);

        let cmd3 = parse_ssh_command("git-receive-pack 'owner/repo.git'").unwrap();
        assert_ne!(cmd1, cmd3);
    }

    #[test]
    fn parsed_command_clone() {
        let cmd = parse_ssh_command("git-upload-pack 'owner/repo.git'").unwrap();
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    #[test]
    fn parsed_command_debug_format() {
        let cmd = parse_ssh_command("git-upload-pack 'owner/repo.git'").unwrap();
        let debug = format!("{cmd:?}");
        assert!(debug.contains("owner"));
        assert!(debug.contains("repo"));
    }
}
