#!/usr/bin/env bash
# detect-worktree.sh — Output the worktree name for namespace scoping.
#
# If inside .claude/worktrees/<name>, outputs <name>. Otherwise outputs "main".

set -euo pipefail

TOPLEVEL="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

if [[ "$TOPLEVEL" =~ /.claude/worktrees/([^/]+)$ ]]; then
  echo "${BASH_REMATCH[1]}"
else
  echo "main"
fi
