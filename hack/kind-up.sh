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

echo ""
echo "Kind cluster ready."
echo "  kubectl context set to: kind-${CLUSTER_NAME}"
echo "  Next: just dev-env"
