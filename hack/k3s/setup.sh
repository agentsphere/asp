#!/usr/bin/env bash
# setup.sh — One-time bootstrap for the k3s dev environment on a VPS.
#
# Usage:
#   bash hack/k3s/setup.sh                    # default namespace: platform-dev
#   bash hack/k3s/setup.sh platform-dev-2     # custom namespace (second instance)
#
# Prerequisites:
#   - Linux x86_64 VPS with root access
#   - Internet connectivity (pulls k3s, container images)

set -euo pipefail

NS="${1:-platform-dev}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# ── Install k3s if not present ─────────────────────────────────────
if ! command -v k3s &>/dev/null; then
  echo "==> Installing k3s (no Traefik)"
  curl -sfL https://get.k3s.io | INSTALL_K3S_EXEC="--disable traefik" sh -
  echo "  Waiting for k3s to be ready..."
  sleep 5
  kubectl wait --for=condition=Ready node --all --timeout=60s
fi

export KUBECONFIG=/etc/rancher/k3s/k3s.yaml

# ── Shared directory for E2E git repos ─────────────────────────────
mkdir -p /tmp/platform-e2e

# ── Apply manifests ────────────────────────────────────────────────
echo "==> Deploying dev environment in namespace: ${NS}"
if [[ "$NS" != "platform-dev" ]]; then
  sed "s/platform-dev/${NS}/g" "${SCRIPT_DIR}/dev-env.yaml" | kubectl apply -f -
else
  kubectl apply -f "${SCRIPT_DIR}/dev-env.yaml"
fi

# ── Wait for services ──────────────────────────────────────────────
echo "==> Waiting for services..."
kubectl wait -n "${NS}" --for=condition=Available deploy/postgres --timeout=120s
kubectl wait -n "${NS}" --for=condition=Available deploy/valkey --timeout=60s
kubectl wait -n "${NS}" --for=condition=Available deploy/minio --timeout=60s

# ── Post-deploy: CREATEDB + MinIO buckets ──────────────────────────
echo "==> Post-deploy setup"

# Verify CREATEDB privilege (required by #[sqlx::test] macro).
# POSTGRES_USER=platform is the superuser, so it already has CREATEDB.
kubectl exec -n "${NS}" deploy/postgres -- \
  psql -U platform -d platform_dev -c "SELECT 1;" -q

# Create MinIO buckets
sleep 2
kubectl exec -n "${NS}" deploy/minio -- sh -c \
  'mc alias set local http://localhost:9000 platform devdevdev 2>/dev/null; \
   mc mb local/platform --ignore-existing; \
   mc mb local/platform-e2e --ignore-existing'

# ── Git SSH key ────────────────────────────────────────────────────
if ! kubectl get secret git-ssh-key -n "${NS}" &>/dev/null; then
  echo ""
  echo "==> Git SSH key setup"
  echo "  Path to SSH private key (default: ~/.ssh/id_rsa, empty to skip):"
  read -r KEY_PATH
  if [[ -n "${KEY_PATH:-}" || -f "$HOME/.ssh/id_rsa" ]]; then
    KEY_PATH="${KEY_PATH:-$HOME/.ssh/id_rsa}"
    kubectl create secret generic git-ssh-key -n "${NS}" \
      --from-file=id_rsa="${KEY_PATH}" \
      --from-file=id_rsa.pub="${KEY_PATH}.pub" \
      --from-file=known_hosts=<(ssh-keyscan github.com 2>/dev/null)
    echo "  Secret created."
  else
    echo "  Skipped (no key found)."
  fi
fi

# ── Claude Code credentials (optional — skip if using Max login) ──
if ! kubectl get secret claude-credentials -n "${NS}" &>/dev/null; then
  echo ""
  echo "  ANTHROPIC_API_KEY (press Enter to skip if using Claude Max login):"
  read -rs API_KEY
  if [[ -n "${API_KEY}" ]]; then
    kubectl create secret generic claude-credentials -n "${NS}" \
      --from-literal=api-key="${API_KEY}"
    echo "  Secret created."
  else
    echo "  Skipped (use 'claude login' inside the pod for Max subscription)."
  fi
fi

# ── Wait for dev pod ───────────────────────────────────────────────
echo "==> Waiting for dev pod..."
kubectl wait -n "${NS}" pod/dev --for=condition=Ready --timeout=300s

echo ""
echo "================================================================"
echo "Dev environment ready in namespace: ${NS}"
echo ""
echo "  kubectl exec -it -n ${NS} dev -- bash"
echo ""
echo "Inside the pod:"
echo "  cd /workspace"
echo "  git clone <your-repo-url> platform"
echo "  cd platform"
echo "  just test-unit        # sanity check (no infra needed)"
echo "  just test-integration # uses ephemeral K8s namespaces"
echo "  just ci-full          # full CI gate"
echo ""
echo "First-time setup:"
echo "  claude login           # OAuth login (Max subscription)"
echo "  ssh -T git@github.com  # verify SSH"
echo "================================================================"
