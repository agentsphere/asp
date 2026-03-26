#!/usr/bin/env bash
# debug-claude-validate.sh — Reproduce the token validation step.
#
# This runs `claude --print` with CLAUDE_CODE_OAUTH_TOKEN set, exactly
# as the platform does in run_cli_validation(), and shows the raw NDJSON.
#
# Usage:
#   bash hack/debug-claude-validate.sh <oauth-token>
#   bash hack/debug-claude-validate.sh <oauth-token> /path/to/claude

set -euo pipefail

TOKEN="${1:?Usage: $0 <oauth-token> [claude-path]}"
CLAUDE_CLI="${2:-claude}"
CONFIG_DIR=$(mktemp -d /tmp/platform-claude-validate-XXXXXX)

echo "==> Claude CLI: $(which "$CLAUDE_CLI" 2>/dev/null || echo "$CLAUDE_CLI")"
echo "==> Config dir: $CONFIG_DIR"
echo "==> Token prefix: ${TOKEN:0:20}..."
echo ""

cleanup() {
  rm -rf "$CONFIG_DIR"
}
trap cleanup EXIT

echo "==> Running: $CLAUDE_CLI --print --output-format stream-json --verbose --max-turns 1 hi"
echo "==> (with env_clear + CLAUDE_CODE_OAUTH_TOKEN + CLAUDE_CONFIG_DIR)"
echo ""
echo "--- STDOUT (NDJSON) ---"

# Match exactly what run_cli_validation does: env_clear + minimal env
env -i \
  PATH="$PATH" \
  HOME="$HOME" \
  TMPDIR="${TMPDIR:-/tmp}" \
  CLAUDE_CONFIG_DIR="$CONFIG_DIR" \
  CLAUDE_CODE_OAUTH_TOKEN="$TOKEN" \
  "$CLAUDE_CLI" --print --output-format stream-json --verbose --max-turns 1 "hi" 2>/tmp/claude-validate-stderr.txt \
  | tee /tmp/claude-validate-stdout.txt

EXIT_CODE=${PIPESTATUS[0]}

echo ""
echo "--- STDERR ---"
cat /tmp/claude-validate-stderr.txt

echo ""
echo "==> Exit code: $EXIT_CODE"
echo "==> Stdout lines: $(wc -l < /tmp/claude-validate-stdout.txt)"
echo ""
echo "==> Parsing NDJSON for system.init and result:"
while IFS= read -r line; do
  type=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('type',''))" 2>/dev/null || echo "?")
  subtype=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('subtype',''))" 2>/dev/null || echo "")
  error=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('error',''))" 2>/dev/null || echo "")
  is_error=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('is_error',''))" 2>/dev/null || echo "")
  if [ -n "$type" ] && [ "$type" != "?" ]; then
    echo "  type=$type subtype=$subtype error=$error is_error=$is_error"
  fi
done < /tmp/claude-validate-stdout.txt

echo ""
echo "==> Platform would return: "
if grep -q '"error":"authentication_failed"' /tmp/claude-validate-stdout.txt; then
  echo "  INVALID (authentication_failed found)"
elif grep -q '"subtype":"init"' /tmp/claude-validate-stdout.txt; then
  echo "  VALID (system.init found)"
else
  echo "  INVALID (no system.init found)"
fi
