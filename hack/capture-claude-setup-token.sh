#!/usr/bin/env bash
# capture-claude-setup-token.sh — Capture the FULL `claude setup-token` flow
# byte-for-byte, including the interactive stdin/stdout exchange.
#
# This captures TWO phases:
#   Phase 1: CLI starts → emits banner + OAuth URL → waits for auth code on stdin
#   Phase 2: User pastes auth code → CLI emits success message + OAuth token → exits
#
# Output files (all saved to hack/claude-capture/):
#   phase1-raw.bin    — raw PTY bytes from startup to "waiting for code" prompt
#   phase2-raw.bin    — raw PTY bytes after code is entered until CLI exits
#   full-raw.bin      — complete raw capture (phase1 + stdin + phase2)
#   phase1-clean.txt  — ANSI-stripped phase 1 text
#   phase2-clean.txt  — ANSI-stripped phase 2 text
#   full-clean.txt    — ANSI-stripped full output
#   metadata.json     — timing, CLI version, extracted URL + token
#
# Usage:
#   bash hack/capture-claude-setup-token.sh
#   bash hack/capture-claude-setup-token.sh /path/to/claude

set -euo pipefail

CLAUDE_CLI="${1:-claude}"
CAPTURE_DIR="$(cd "$(dirname "$0")/.." && pwd)/hack/claude-capture"
CONFIG_DIR=$(mktemp -d /tmp/platform-claude-capture-XXXXXX)

mkdir -p "$CAPTURE_DIR"

echo "==> Claude CLI: $(which "$CLAUDE_CLI" 2>/dev/null || echo "$CLAUDE_CLI")"
echo "==> Capture dir: $CAPTURE_DIR"
echo "==> Temp config: $CONFIG_DIR"
echo ""

# Create a helper script that the PTY will run.
# It captures stdout in phases using a named pipe for coordination.
HELPER="$CONFIG_DIR/helper.sh"
PHASE1_RAW="$CAPTURE_DIR/phase1-raw.bin"
PHASE2_RAW="$CAPTURE_DIR/phase2-raw.bin"
FULL_RAW="$CAPTURE_DIR/full-raw.bin"
CODE_PIPE="$CONFIG_DIR/code-pipe"

mkfifo "$CODE_PIPE"

cat > "$HELPER" << 'HELPEREOF'
#!/usr/bin/env bash
# This runs inside the PTY. It:
# 1. Starts claude setup-token with stdout piped through tee
# 2. Waits for the auth code from the parent via a named pipe
# 3. Feeds the code to claude's stdin
set -euo pipefail

CLAUDE_CLI="$1"
PHASE1_RAW="$2"
PHASE2_RAW="$3"
FULL_RAW="$4"
CODE_PIPE="$5"
CONFIG_DIR="$6"

stty columns 500 2>/dev/null

# Start claude setup-token, capturing all output
# Use coproc to get stdin/stdout handles
coproc CLAUDE { exec "$CLAUDE_CLI" setup-token 2>/dev/null; }

# Read phase 1 output (until we detect the URL prompt)
# Accumulate all output, tee to full capture
{
  while IFS= read -r -d '' -n 1 char || [ -n "$char" ]; do
    printf '%s' "$char"
    printf '%s' "$char" >> "$PHASE1_RAW"
    printf '%s' "$char" >> "$FULL_RAW"
  done
} <&"${CLAUDE[0]}" &
READER_PID=$!

# Wait for user to complete OAuth in browser, then read auth code from pipe
AUTH_CODE=$(cat "$CODE_PIPE")

# Kill the background reader — we'll start a new one for phase 2
kill $READER_PID 2>/dev/null || true
wait $READER_PID 2>/dev/null || true

# Feed auth code to claude's stdin
echo "$AUTH_CODE" >&"${CLAUDE[1]}"
printf '%s\n' "$AUTH_CODE" >> "$FULL_RAW"

# Read phase 2 output until claude exits
{
  while IFS= read -r -d '' -n 1 char || [ -n "$char" ]; do
    printf '%s' "$char"
    printf '%s' "$char" >> "$PHASE2_RAW"
    printf '%s' "$char" >> "$FULL_RAW"
  done
} <&"${CLAUDE[0]}" || true

wait "${CLAUDE_PID}" 2>/dev/null || true
HELPEREOF
chmod +x "$HELPER"

# Simpler approach: just capture everything and split later
# The PTY makes it hard to do phase splitting inside, so let's
# capture the full raw stream and interact manually.

cleanup() {
  rm -rf "$CONFIG_DIR"
}
trap cleanup EXIT

echo "==> Starting claude setup-token via PTY..."
echo "==> Complete the OAuth flow in your browser."
echo "==> When you see the URL, go to it and authorize."
echo "==> The CLI will automatically detect completion and show the token."
echo "==> (If it asks for a code, paste it.)"
echo ""
echo "=========================================="
echo ""

# Clear output files
> "$PHASE1_RAW"
> "$PHASE2_RAW"
> "$FULL_RAW"

# Use the exact same PTY spawn as the platform (spawn_claude_setup_token)
ESCAPED_CLI=$(printf '%s' "$CLAUDE_CLI" | sed "s/'/'\\\\''/g")
CMD="stty columns 500 2>/dev/null; exec '${ESCAPED_CLI}' setup-token"

# Capture everything via script PTY wrapper
CLAUDE_CONFIG_DIR="$CONFIG_DIR" \
  script -q /dev/null bash -c "$CMD" 2>/dev/null | tee "$FULL_RAW"

echo ""
echo "=========================================="
echo ""
echo "==> CLI exited. Processing captured output..."
echo ""

# Strip ANSI escapes (mirrors platform's strip_ansi_escapes exactly)
strip_ansi() {
  perl -pe '
    s/\e\[[0-9;]*[A-Za-z]//g;   # CSI: ESC [ params letter
    s/\e\][^\a]*(?:\a|\e\\)//g;  # OSC: ESC ] ... BEL/ST
    s/\e[^[\]][^\a]//g;          # Other 2-byte ESC seqs
    s/\r//g;                      # Carriage returns
    s/\x00//g;                    # Null bytes
  '
}

FULL_CLEAN="$CAPTURE_DIR/full-clean.txt"
strip_ansi < "$FULL_RAW" > "$FULL_CLEAN"

# Split clean output into phases at the token line
# Phase 1: everything up to and including the URL
# Phase 2: everything from "Long-lived" or "token created" onward
PHASE1_CLEAN="$CAPTURE_DIR/phase1-clean.txt"
PHASE2_CLEAN="$CAPTURE_DIR/phase2-clean.txt"

# Find the line containing the OAuth URL
URL_LINE=$(grep -n 'https://claude\.\(ai\|com\)' "$FULL_CLEAN" | head -1 | cut -d: -f1 || echo "")
if [ -n "$URL_LINE" ]; then
  # Phase 1: up to 5 lines after URL (includes the "paste code" prompt)
  head -n "$((URL_LINE + 5))" "$FULL_CLEAN" > "$PHASE1_CLEAN"
  tail -n +"$((URL_LINE + 6))" "$FULL_CLEAN" > "$PHASE2_CLEAN"
else
  cp "$FULL_CLEAN" "$PHASE1_CLEAN"
  > "$PHASE2_CLEAN"
fi

# Extract URL and token from clean output
URL=$(grep -oE 'https://claude\.(ai|com)/(cai/)?oauth/authorize\?[A-Za-z0-9_.~:/?#@!$&'"'"'()*+,;=%=-]+' "$FULL_CLEAN" | head -1 || true)
TOKEN=$(grep -oE 'sk-ant-oat[A-Za-z0-9_-]+' "$FULL_CLEAN" | head -1 || true)

# CLI version
VERSION=$(grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+' "$FULL_CLEAN" | head -1 || echo "unknown")

# Write metadata
cat > "$CAPTURE_DIR/metadata.json" << METAEOF
{
  "captured_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "cli_version": "$VERSION",
  "cli_path": "$(which "$CLAUDE_CLI" 2>/dev/null || echo "$CLAUDE_CLI")",
  "full_raw_bytes": $(wc -c < "$FULL_RAW"),
  "full_clean_bytes": $(wc -c < "$FULL_CLEAN"),
  "url_extracted": $([ -n "$URL" ] && echo "true" || echo "false"),
  "url_length": ${#URL},
  "url_prefix": "$(echo "$URL" | head -c 60)...",
  "token_extracted": $([ -n "$TOKEN" ] && echo "true" || echo "false"),
  "token_length": ${#TOKEN},
  "token_prefix": "$(echo "$TOKEN" | head -c 20)..."
}
METAEOF

echo ""
echo "==> RESULTS"
echo "==========="
echo ""
echo "Files saved to: $CAPTURE_DIR/"
ls -la "$CAPTURE_DIR/"
echo ""

if [ -n "$URL" ]; then
  echo "==> URL extracted (${#URL} chars): ${URL:0:80}..."
else
  echo "==> WARNING: No OAuth URL found!"
fi

if [ -n "$TOKEN" ]; then
  echo "==> Token extracted (${#TOKEN} chars): ${TOKEN:0:20}...${TOKEN: -10}"
else
  echo "==> WARNING: No OAuth token found!"
fi

echo ""
echo "==> Phase 1 clean output (banner + URL):"
echo "---"
cat "$PHASE1_CLEAN"
echo "---"
echo ""
echo "==> Phase 2 clean output (after code, token):"
echo "---"
cat "$PHASE2_CLEAN"
echo "---"
echo ""
echo "==> To inspect raw bytes: xxd $FULL_RAW | less"
echo "==> To build mock: use $CAPTURE_DIR/phase1-clean.txt and phase2-clean.txt"
