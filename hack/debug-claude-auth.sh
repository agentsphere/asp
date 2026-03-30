#!/usr/bin/env bash
# debug-claude-auth.sh — Capture exact bytes from `claude setup-token` as the
# platform sees them (via PTY wrapper), then run the same ANSI-strip + URL/token
# extraction logic the platform uses.
#
# Purpose: understand the real CLI output format so we can build an accurate mock.
#
# Usage:
#   bash hack/debug-claude-auth.sh              # uses `claude` from PATH
#   bash hack/debug-claude-auth.sh /path/to/claude
#
# Output files (kept after exit):
#   /tmp/platform-claude-raw-*.txt   — raw bytes from PTY stdout
#   /tmp/platform-claude-clean-*.txt — after ANSI stripping (what platform parses)

set -euo pipefail

CLAUDE_CLI="${1:-claude}"
CONFIG_DIR=$(mktemp -d /tmp/platform-claude-debug-XXXXXX)
RAW_OUTPUT=$(mktemp /tmp/platform-claude-raw-XXXXXX.txt)
CLEAN_OUTPUT="${RAW_OUTPUT%.txt}-clean.txt"

echo "==> Claude CLI: $(which "$CLAUDE_CLI" 2>/dev/null || echo "$CLAUDE_CLI")"
echo "==> Config dir: $CONFIG_DIR"
echo "==> Raw output: $RAW_OUTPUT"
echo "==> Clean output: $CLEAN_OUTPUT"
echo ""

cleanup() {
  rm -rf "$CONFIG_DIR"
  echo ""
  echo "========================================================================"
  echo "==> Raw output saved to: $RAW_OUTPUT"
  echo "==> Raw size: $(wc -c < "$RAW_OUTPUT") bytes"
  echo ""

  # Strip ANSI escapes (same logic as platform's strip_ansi_escapes)
  # Uses perl to remove CSI sequences (ESC[...letter) and OSC/other ESC sequences
  perl -pe '
    s/\e\[[0-9;]*[A-Za-z]//g;   # CSI sequences: ESC [ params letter
    s/\e\][^\a]*(\a|\e\\)//g;   # OSC sequences: ESC ] ... BEL/ST
    s/\e[^[\]].//g;             # Other 2-byte ESC sequences
    s/\r//g;                     # Strip carriage returns
  ' "$RAW_OUTPUT" > "$CLEAN_OUTPUT"

  echo "==> Clean output saved to: $CLEAN_OUTPUT"
  echo "==> Clean size: $(wc -c < "$CLEAN_OUTPUT") bytes"
  echo ""

  echo "--- CLEAN TEXT (ANSI stripped) ---"
  cat "$CLEAN_OUTPUT"
  echo ""
  echo "--- END CLEAN TEXT ---"
  echo ""

  # Extract URL (same pattern as platform's find_oauth_url)
  URL=$(grep -oE 'https://claude\.(ai|com)/(cai/)?oauth/authorize\?[^ ]+' "$CLEAN_OUTPUT" | head -1 || true)
  if [ -n "$URL" ]; then
    echo "==> EXTRACTED URL: $URL"
    echo "==> URL length: ${#URL}"
  else
    echo "==> WARNING: No OAuth URL found in clean output!"
  fi
  echo ""

  # Extract token (same pattern as platform's find_oauth_token)
  TOKEN=$(grep -oE 'sk-ant-oat[A-Za-z0-9_-]+' "$CLEAN_OUTPUT" | head -1 || true)
  if [ -n "$TOKEN" ]; then
    echo "==> EXTRACTED TOKEN: ${TOKEN:0:20}...${TOKEN: -10} (${#TOKEN} chars)"
  else
    echo "==> WARNING: No OAuth token found in clean output!"
  fi
  echo ""

  echo "--- HEX DUMP (first 2000 bytes of raw) ---"
  xxd "$RAW_OUTPUT" | head -60
}
trap cleanup EXIT

# This is the exact same command the platform uses (see spawn_claude_setup_token)
ESCAPED_CLI=$(printf '%s' "$CLAUDE_CLI" | sed "s/'/'\\\\''/g")
CMD="stty columns 500 2>/dev/null; exec '${ESCAPED_CLI}' setup-token"

echo "==> Spawning: script -q /dev/null bash -c \"$CMD\""
echo "==> (with CLAUDE_CONFIG_DIR=$CONFIG_DIR)"
echo ""
echo "--- RAW STDOUT (press Ctrl+C when done, or wait for completion) ---"

# Spawn via script PTY wrapper, same as platform
CLAUDE_CONFIG_DIR="$CONFIG_DIR" \
  script -q /dev/null bash -c "$CMD" 2>/dev/null | tee "$RAW_OUTPUT"
