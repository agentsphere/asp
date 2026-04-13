// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Git hook types and pkt-line parsing.

use std::path::PathBuf;

use uuid::Uuid;

/// A single ref update from a push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefUpdate {
    pub old_sha: String,
    pub new_sha: String,
    pub refname: String,
}

/// Parameters for post-receive processing.
pub struct PostReceiveParams {
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub user_name: String,
    pub repo_path: PathBuf,
    pub default_branch: String,
    /// Branch names that were updated (stripped of `refs/heads/` prefix).
    pub pushed_branches: Vec<String>,
    /// Tag names that were pushed (stripped of `refs/tags/` prefix).
    pub pushed_tags: Vec<String>,
}

/// Extract branch names from ref updates, filtering to `refs/heads/*` only.
///
/// Strips the `refs/heads/` prefix and skips deletions (new_sha all zeros).
pub fn extract_pushed_branches(updates: &[RefUpdate]) -> Vec<String> {
    let zero_sha = "0".repeat(40);
    updates
        .iter()
        .filter_map(|u| {
            if u.new_sha == zero_sha {
                return None;
            }
            u.refname.strip_prefix("refs/heads/").map(str::to_string)
        })
        .collect()
}

/// Extract tag names from ref updates, filtering to `refs/tags/*` only.
///
/// Strips the `refs/tags/` prefix, skips deletions, and rejects dangerous characters.
pub fn extract_pushed_tags(updates: &[RefUpdate]) -> Vec<String> {
    let zero_sha = "0".repeat(40);
    updates
        .iter()
        .filter_map(|u| {
            if u.new_sha == zero_sha {
                return None;
            }
            let tag = u.refname.strip_prefix("refs/tags/")?;
            if tag.contains("..")
                || tag.contains('\0')
                || tag.contains(';')
                || tag.contains('|')
                || tag.contains('$')
                || tag.contains('`')
            {
                tracing::warn!(tag, "rejected tag with dangerous characters");
                return None;
            }
            Some(tag.to_string())
        })
        .collect()
}

/// Parse ref update commands from a git receive-pack request body (pkt-line format).
pub fn parse_pack_commands(data: &[u8]) -> Vec<RefUpdate> {
    let mut updates = Vec::new();
    let mut pos = 0;

    while pos + 4 <= data.len() {
        let Ok(len_hex) = std::str::from_utf8(&data[pos..pos + 4]) else {
            break;
        };

        if len_hex == "0000" {
            break;
        }

        let pkt_len = match usize::from_str_radix(len_hex, 16) {
            Ok(n) if n >= 4 => n,
            _ => break,
        };

        if pos + pkt_len > data.len() {
            break;
        }

        let line_bytes = &data[pos + 4..pos + pkt_len];

        if let Ok(line) = std::str::from_utf8(line_bytes) {
            let line = line.split('\0').next().unwrap_or(line).trim();
            let mut parts = line.splitn(3, ' ');
            if let (Some(old_sha), Some(new_sha), Some(refname)) =
                (parts.next(), parts.next(), parts.next())
                && old_sha.len() >= 40
                && new_sha.len() >= 40
                && !refname.is_empty()
            {
                updates.push(RefUpdate {
                    old_sha: old_sha.to_owned(),
                    new_sha: new_sha.to_owned(),
                    refname: refname.to_owned(),
                });
            }
        }

        pos += pkt_len;
    }

    updates
}

/// Parse ref update lines from receive-pack output (test helper).
///
/// Each line has the format: `old_sha new_sha refname\n`
pub fn parse_ref_updates(input: &str) -> Vec<RefUpdate> {
    input
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let mut parts = line.splitn(3, ' ');
            let old_sha = parts.next()?.to_owned();
            let new_sha = parts.next()?.to_owned();
            let refname = parts.next()?.to_owned();
            if old_sha.len() < 40 || new_sha.len() < 40 || refname.is_empty() {
                return None;
            }
            Some(RefUpdate {
                old_sha,
                new_sha,
                refname,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_normal_push() {
        let sha = "a".repeat(40);
        let sha2 = "b".repeat(40);
        let input = format!("{sha} {sha2} refs/heads/main\n");
        let updates = parse_ref_updates(&input);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].refname, "refs/heads/main");
    }

    #[test]
    fn parse_empty_input() {
        assert!(parse_ref_updates("").is_empty());
        assert!(parse_ref_updates("  \n  \n").is_empty());
    }

    #[test]
    fn extract_branches_from_updates() {
        let updates = vec![
            RefUpdate {
                old_sha: "a".repeat(40),
                new_sha: "b".repeat(40),
                refname: "refs/heads/main".into(),
            },
            RefUpdate {
                old_sha: "a".repeat(40),
                new_sha: "b".repeat(40),
                refname: "refs/heads/feature/login".into(),
            },
        ];
        let branches = extract_pushed_branches(&updates);
        assert_eq!(branches, vec!["main", "feature/login"]);
    }

    #[test]
    fn extract_branches_skips_deletions() {
        let updates = vec![RefUpdate {
            old_sha: "a".repeat(40),
            new_sha: "0".repeat(40),
            refname: "refs/heads/old-branch".into(),
        }];
        assert!(extract_pushed_branches(&updates).is_empty());
    }

    #[test]
    fn extract_branches_skips_tags() {
        let updates = vec![RefUpdate {
            old_sha: "a".repeat(40),
            new_sha: "b".repeat(40),
            refname: "refs/tags/v1.0.0".into(),
        }];
        assert!(extract_pushed_branches(&updates).is_empty());
    }

    #[test]
    fn extract_tags_valid() {
        let updates = vec![RefUpdate {
            old_sha: "a".repeat(40),
            new_sha: "b".repeat(40),
            refname: "refs/tags/v1.0.0".into(),
        }];
        let tags = extract_pushed_tags(&updates);
        assert_eq!(tags, vec!["v1.0.0"]);
    }

    #[test]
    fn extract_tags_rejects_dangerous() {
        for dangerous in &["v1..0", "v1\0x", "v1;rm", "v1|cat", "$HOME", "v1`cmd`"] {
            let updates = vec![RefUpdate {
                old_sha: "a".repeat(40),
                new_sha: "b".repeat(40),
                refname: format!("refs/tags/{dangerous}"),
            }];
            assert!(
                extract_pushed_tags(&updates).is_empty(),
                "should reject: {dangerous}"
            );
        }
    }

    #[test]
    fn extract_tags_allows_safe_special_chars() {
        let updates = vec![
            RefUpdate {
                old_sha: "a".repeat(40),
                new_sha: "b".repeat(40),
                refname: "refs/tags/v1.0.0-beta+build.123".into(),
            },
            RefUpdate {
                old_sha: "a".repeat(40),
                new_sha: "b".repeat(40),
                refname: "refs/tags/release/v2.0".into(),
            },
        ];
        let tags = extract_pushed_tags(&updates);
        assert_eq!(tags, vec!["v1.0.0-beta+build.123", "release/v2.0"]);
    }

    #[test]
    fn parse_pack_single_ref() {
        let old = "a".repeat(40);
        let new = "b".repeat(40);
        let cmd = format!("{old} {new} refs/heads/main\0 report-status\n");
        let pkt_len = cmd.len() + 4;
        let data = format!("{pkt_len:04x}{cmd}0000");
        let updates = parse_pack_commands(data.as_bytes());
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].refname, "refs/heads/main");
    }

    #[test]
    fn parse_pack_empty_data() {
        assert!(parse_pack_commands(b"0000").is_empty());
        assert!(parse_pack_commands(b"").is_empty());
    }

    #[test]
    fn parse_pack_multiple_refs() {
        let old = "a".repeat(40);
        let new = "b".repeat(40);
        let cmd1 = format!("{old} {new} refs/heads/main\0 report-status\n");
        let cmd2 = format!("{old} {new} refs/heads/feature\n");
        let len1 = cmd1.len() + 4;
        let len2 = cmd2.len() + 4;
        let data = format!("{len1:04x}{cmd1}{len2:04x}{cmd2}0000");
        let updates = parse_pack_commands(data.as_bytes());
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].refname, "refs/heads/main");
        assert_eq!(updates[1].refname, "refs/heads/feature");
    }

    #[test]
    fn parse_pack_invalid_hex() {
        assert!(parse_pack_commands(b"zzzz some data here").is_empty());
    }

    #[test]
    fn parse_pack_incomplete() {
        assert!(parse_pack_commands(b"00c8short data").is_empty());
    }

    #[test]
    fn ref_update_equality() {
        let a = RefUpdate {
            old_sha: "a".repeat(40),
            new_sha: "b".repeat(40),
            refname: "refs/heads/main".into(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn post_receive_params_struct() {
        let params = PostReceiveParams {
            project_id: Uuid::nil(),
            user_id: Uuid::nil(),
            user_name: "alice".into(),
            repo_path: PathBuf::from("/repos/test.git"),
            default_branch: "main".into(),
            pushed_branches: vec!["main".into()],
            pushed_tags: vec!["v1.0.0".into()],
        };
        assert_eq!(params.pushed_branches.len(), 1);
        assert_eq!(params.pushed_tags.len(), 1);
    }
}
