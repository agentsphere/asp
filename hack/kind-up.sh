#!/usr/bin/env bash
set -euo pipefail

CLUSTER_NAME="platform"

# Create cluster if it doesn't exist
if ! kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
  kind create cluster --name "$CLUSTER_NAME" --config hack/kind-config.yaml
fi

# Export kubeconfig — dedicated file for Justfile, and merge into default ~/.kube/config
# (Justfile sets KUBECONFIG=~/.kube/kind-platform, so we must explicitly target both)
kind get kubeconfig --name "$CLUSTER_NAME" > "${HOME}/.kube/kind-platform"
KUBECONFIG="${HOME}/.kube/config" kind export kubeconfig --name "$CLUSTER_NAME"
export KUBECONFIG="${HOME}/.kube/kind-platform"

# Install CNPG operator (cluster-wide, needed by PG clusters)
helm repo add cnpg https://cloudnative-pg.github.io/charts --force-update
helm upgrade --install cnpg cnpg/cloudnative-pg -n cnpg-system --create-namespace --wait

# Create shared temp directory for e2e test repos (mounted via extraMounts)
mkdir -p /tmp/platform-e2e

# Pre-load socat image for DaemonSet registry proxy
echo "==> Pre-loading socat image"
docker pull alpine/socat:latest 2>/dev/null || true
kind load docker-image alpine/socat:latest --name "$CLUSTER_NAME" 2>/dev/null || true

# Build runner OCI tarball for registry seeding
SEED_DIR="/tmp/platform-e2e/seed-images"
if [[ ! -f "${SEED_DIR}/platform-runner.tar" ]]; then
  echo "==> Building platform-runner OCI tarball"
  mkdir -p "${SEED_DIR}"
  docker buildx build \
    --file docker/Dockerfile.platform-runner-bare \
    --output "type=oci,dest=${SEED_DIR}/platform-runner.tar" \
    .
else
  echo "==> Reusing existing platform-runner OCI tarball"
fi

echo ""
echo "Kind cluster ready."
echo "  kubectl context set to: kind-${CLUSTER_NAME}"
echo "  Next: just dev-env"
