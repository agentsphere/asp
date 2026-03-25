#!/usr/bin/env bash
# Mock Claude CLI for integration tests.
#
# Emits NDJSON system → assistant → result messages based on environment:
#   MOCK_CLI_RESPONSE_FILE  — JSON file with array of response objects (one per invocation)
#   MOCK_CLI_STATE_DIR      — directory for tracking invocation count
#
# Each response object in the file has:
#   { "text": "...", "tools": [...], "session_id": "..." }
#
# Falls back to a default "Hello!" text-only response if no file is set.
set -euo pipefail

# Parse args for session-id or resume (used in emitted messages)
SESSION_ID="mock-session-id"
PROMPT=""
IS_RESUME=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --session-id) SESSION_ID="$2"; shift 2 ;;
        --resume) SESSION_ID="$2"; IS_RESUME=true; shift 2 ;;
        -p) PROMPT="$2"; shift 2 ;;
        *) shift ;;
    esac
done

# Track invocation count
STATE_DIR="${MOCK_CLI_STATE_DIR:-/tmp/mock-cli-state}"
mkdir -p "$STATE_DIR"
COUNT_FILE="$STATE_DIR/invocation-count"
COUNT=$(cat "$COUNT_FILE" 2>/dev/null || echo "0")
echo $((COUNT + 1)) > "$COUNT_FILE"

# Record invocation for test assertions
echo "$COUNT $PROMPT" >> "$STATE_DIR/invocations.log"

# Read response from file or use default
TEXT="Hello!"
TOOLS="[]"
if [[ -n "${MOCK_CLI_RESPONSE_FILE:-}" ]] && [[ -f "$MOCK_CLI_RESPONSE_FILE" ]]; then
    # Extract the $COUNT-th element from the JSON array
    ENTRY=$(python3 -c "
import json, sys
with open('$MOCK_CLI_RESPONSE_FILE') as f:
    data = json.load(f)
idx = $COUNT
if idx < len(data):
    print(json.dumps(data[idx]))
else:
    print(json.dumps(data[-1]))  # repeat last response if out of bounds
" 2>/dev/null || echo '{"text":"Hello!","tools":[]}')
    TEXT=$(echo "$ENTRY" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('text','Hello!'))")
    TOOLS=$(echo "$ENTRY" | python3 -c "import json,sys; d=json.load(sys.stdin); print(json.dumps(d.get('tools',[])))")
fi

# Emit NDJSON: system init
echo "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\",\"tools\":[\"StructuredOutput\"],\"model\":\"claude-sonnet-4-20250514\",\"claude_code_version\":\"1.0.0-mock\"}"

# Emit NDJSON: assistant thinking
echo "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"thinking\",\"thinking\":\"Let me analyze the request...\"}]},\"session_id\":\"$SESSION_ID\"}"

# Emit NDJSON: assistant tool_use
echo "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"read_file\",\"input\":{\"path\":\"src/main.rs\"}}]},\"session_id\":\"$SESSION_ID\"}"

# Emit NDJSON: assistant tool_result
echo "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"content\":\"file contents here\"}]},\"session_id\":\"$SESSION_ID\"}"

# Emit NDJSON: assistant text message
echo "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":$(python3 -c "import json; print(json.dumps('$TEXT'))" 2>/dev/null || echo "\"$TEXT\"")}]},\"session_id\":\"$SESSION_ID\"}"

# Emit NDJSON: error event
echo "{\"type\":\"error\",\"error\":{\"message\":\"test error\"}}"

# Emit NDJSON: result with structured_output, cost, and duration
STRUCTURED=$(python3 -c "
import json
text = $(python3 -c "import json; print(json.dumps('$TEXT'))" 2>/dev/null || echo "'$TEXT'")
tools = json.loads('$TOOLS')
print(json.dumps({'text': text, 'tools': tools}))
" 2>/dev/null || echo "{\"text\":\"$TEXT\",\"tools\":$TOOLS}")

echo "{\"type\":\"result\",\"subtype\":\"success\",\"session_id\":\"$SESSION_ID\",\"is_error\":false,\"result\":$(python3 -c "import json; print(json.dumps('$TEXT'))" 2>/dev/null || echo "\"$TEXT\""),\"usage\":{\"input_tokens\":100,\"output_tokens\":50},\"total_cost_usd\":0.0045,\"duration_ms\":1234,\"num_turns\":3,\"structured_output\":$STRUCTURED}"
