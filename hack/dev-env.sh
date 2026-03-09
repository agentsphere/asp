#!/usr/bin/env bash
# dev-env.sh — Deploy services into Kind and port-forward to fixed ports.
#
# Deploys Postgres/Valkey/MinIO into the Kind cluster's `platform` namespace,
# kills anything occupying the fixed ports, starts background port-forwards,
# verifies connectivity, and exits. Port-forwards survive after the script exits.
#
# Workflow:
#   1. just cluster-up   # create Kind cluster (one-time)
#   2. just dev-env       # this script: deploy + port-forward (exits when ready)
#   3. just run           # cargo run (loads .env.dev automatically)
#
# To stop port-forwards:  just dev-env-stop
#
# Fixed ports (from .env.dev):
#   Postgres  → localhost:5432
#   Valkey    → localhost:6379
#   MinIO     → localhost:9000
#   Platform  → localhost:8080   (via `just run`)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
KIND_CLUSTER="platform"
NODE="${KIND_CLUSTER}-control-plane"
PIDFILE="/tmp/platform-dev-pf.pids"

export KUBECONFIG="${HOME}/.kube/kind-${KIND_CLUSTER}"

# ── Refresh kubeconfig (API server port changes on cluster recreate) ─────
if kind get clusters 2>/dev/null | grep -q "^${KIND_CLUSTER}$"; then
  kind get kubeconfig --name "$KIND_CLUSTER" > "$KUBECONFIG"
  KUBECONFIG="${HOME}/.kube/config" kind export kubeconfig --name "$KIND_CLUSTER"
else
  echo "ERROR: Kind cluster '${KIND_CLUSTER}' not found. Run: just cluster-up"
  exit 1
fi

# ── Fixed ports (must match .env.dev) ────────────────────────────────────
PG_PORT=5432
VALKEY_PORT=6379
MINIO_PORT=9000
BACKEND_PORT=8080
REGISTRY_NODE_PORT=5000

# ── Detect host address ─────────────────────────────────────────────────
if [[ "$(uname)" == "Darwin" ]]; then
  PLATFORM_HOST="host.docker.internal"
else
  PLATFORM_HOST=$(docker network inspect kind \
    -f '{{range .IPAM.Config}}{{.Gateway}}{{end}}' 2>/dev/null || echo "172.18.0.1")
fi

# ── Kill anything on our fixed ports ─────────────────────────────────────
kill_port() {
  local port=$1
  local pids
  pids=$(lsof -ti "tcp:${port}" 2>/dev/null || true)
  if [[ -n "$pids" ]]; then
    echo "  Killing processes on port ${port}: $(echo $pids | tr '\n' ' ')"
    echo "$pids" | xargs kill -9 2>/dev/null || true
  fi
}

echo "==> Clearing fixed ports"

# Kill previously tracked port-forward PIDs
if [[ -f "$PIDFILE" ]]; then
  while read -r pid; do
    kill "$pid" 2>/dev/null || true
  done < "$PIDFILE"
  rm -f "$PIDFILE"
fi

# Kill stale kubectl port-forward processes for our services
pkill -f "kubectl port-forward.*-n platform.*postgres" 2>/dev/null || true
pkill -f "kubectl port-forward.*-n platform.*valkey" 2>/dev/null || true
pkill -f "kubectl port-forward.*-n platform.*minio" 2>/dev/null || true
sleep 0.3

# Kill anything else on the fixed ports
for port in "$PG_PORT" "$VALKEY_PORT" "$MINIO_PORT"; do
  kill_port "$port"
done

# Kill busy port on Kind node for registry proxy
docker exec "$NODE" \
  sh -c "fuser -k ${REGISTRY_NODE_PORT}/tcp 2>/dev/null; true" 2>/dev/null || true

# ── Deploy services + registry proxy (idempotent) ───────────────────────
export REGISTRY_BACKEND_HOST="${PLATFORM_HOST}"
export REGISTRY_BACKEND_PORT="${BACKEND_PORT}"
export REGISTRY_NODE_PORT
bash "${SCRIPT_DIR}/deploy-services.sh" platform

# ── Start background port-forwards ──────────────────────────────────────
echo "==> Starting background port-forwards (fixed ports from .env.dev)"
kubectl port-forward -n platform pod/postgres "${PG_PORT}:5432" &>/dev/null &
PG_PID=$!
kubectl port-forward -n platform pod/valkey "${VALKEY_PORT}:6379" &>/dev/null &
VK_PID=$!
kubectl port-forward -n platform pod/minio "${MINIO_PORT}:9000" &>/dev/null &
MN_PID=$!

# Save PIDs for later cleanup
echo "$PG_PID" > "$PIDFILE"
echo "$VK_PID" >> "$PIDFILE"
echo "$MN_PID" >> "$PIDFILE"

# Detach port-forwards from this shell so they survive script exit
disown "$PG_PID" "$VK_PID" "$MN_PID"

# Wait for port-forwards to be reachable
echo -n "  Waiting for port-forwards"
for i in $(seq 1 30); do
  ALL_READY=true
  for port in "$PG_PORT" "$VALKEY_PORT" "$MINIO_PORT"; do
    if ! nc -z 127.0.0.1 "$port" 2>/dev/null; then
      ALL_READY=false
      break
    fi
  done
  if $ALL_READY; then break; fi
  echo -n "."
  sleep 0.5
done

if ! $ALL_READY; then
  echo " FAILED"
  echo "ERROR: Port-forwards did not become ready in 15s"
  exit 1
fi
echo " ready"

# ── Summary ──────────────────────────────────────────────────────────────
echo ""
echo "==> Dev environment ready"
echo "  Postgres:       localhost:${PG_PORT}  (pid ${PG_PID})"
echo "  Valkey:         localhost:${VALKEY_PORT}  (pid ${VK_PID})"
echo "  MinIO:          localhost:${MINIO_PORT}  (pid ${MN_PID})"
echo "  Registry proxy: Kind node:${REGISTRY_NODE_PORT} → ${PLATFORM_HOST}:${BACKEND_PORT}"
echo ""
echo "  Run: just run"
echo "  Stop port-forwards: just dev-env-stop"
