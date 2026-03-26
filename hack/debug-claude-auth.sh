#!/usr/bin/env bash
# debug-claude-auth.sh — Reproduce exactly what the platform does to extract
# the OAuth URL and token from `claude setup-token`.
#
# This spawns the CLI via the same PTY wrapper (script) and dumps raw output
# so you can see exactly what bytes the platform sees.
#
# Usage:
#   bash hack/debug-claude-auth.sh
#   bash hack/debug-claude-auth.sh /path/to/claude

set -euo pipefail

CLAUDE_CLI="${1:-claude}"
CONFIG_DIR=$(mktemp -d /tmp/platform-claude-debug-XXXXXX)
RAW_OUTPUT=$(mktemp /tmp/platform-claude-raw-XXXXXX.txt)

echo "==> Claude CLI: $(which "$CLAUDE_CLI" 2>/dev/null || echo "$CLAUDE_CLI")"
echo "==> Config dir: $CONFIG_DIR"
echo "==> Raw output: $RAW_OUTPUT"
echo ""

cleanup() {
  rm -rf "$CONFIG_DIR"
  echo ""
  echo "==> Raw output saved to: $RAW_OUTPUT"
  echo "==> Hex dump of first 2000 bytes:"
  xxd "$RAW_OUTPUT" | head -60
}
trap cleanup EXIT

# This is the exact same command the platform uses (see spawn_claude_setup_token)
ESCAPED_CLI=$(printf '%s' "$CLAUDE_CLI" | sed "s/'/'\\\\''/g")
CMD="stty columns 500 2>/dev/null; exec '${ESCAPED_CLI}' setup-token"

echo "==> Spawning: script -q /dev/null bash -c \"$CMD\""
echo "==> (with CLAUDE_CONFIG_DIR=$CONFIG_DIR)"
echo ""
echo "--- RAW STDOUT (press Ctrl+C when done) ---"

# Spawn via script PTY wrapper, same as platform
CLAUDE_CONFIG_DIR="$CONFIG_DIR" \
  script -q /dev/null bash -c "$CMD" 2>/dev/null | tee "$RAW_OUTPUT"
