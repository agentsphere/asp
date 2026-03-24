# 7. Deployment View

## Kind Development Cluster

The development environment uses a single-node Kind cluster with port-mapped services:

<!-- mermaid:diagrams/deployment-kind.mmd -->
```mermaid
flowchart TD
    subgraph Cluster["Development Cluster (Kind)"]
        subgraph Platform["Namespace: platform"]
            pod[Platform<br/>Single binary, port 8080]
            pg[PostgreSQL<br/>Port 5432]
            vk[Valkey<br/>Port 6379]
            minio[MinIO<br/>Ports 9000/9001]
        end

        subgraph Project["Namespace: {slug}-dev"]
            pipeline[Pipeline Pods<br/>Per-step K8s pods]
            agent[Agent Pods<br/>Claude Code sessions]
        end

        subgraph Staging["Namespace: {slug}-staging"]
            stable[Stable<br/>Production image]
            canary[Canary<br/>New image, partial traffic]
        end

        subgraph Test["Namespace: platform-test-*"]
            testinfra[Test Infrastructure<br/>Ephemeral per test run]
        end
    end

    pod --> pg
    pod --> vk
    pod --> minio
    pod --> Project
    pod --> Staging
```
<!-- /mermaid -->

### Port Mappings

| Host Port | Service | Protocol |
|---|---|---|
| 5432 | PostgreSQL | TCP |
| 6379 | Valkey | RESP |
| 8080 | Platform (HTTP + Git + Registry) | HTTP |
| 9000 | MinIO (S3 API) | HTTP |
| 9001 | MinIO (Console) | HTTP |
| 2222 | Platform (Git SSH) | SSH |

### Setup

```bash
just cluster-up     # Creates Kind cluster + deploys Postgres, Valkey, MinIO
just cluster-down   # Destroys cluster
```

## Namespace Strategy

The platform uses per-project K8s namespaces for tenant isolation:

| Namespace Pattern | Purpose | Created By |
|---|---|---|
| `platform` | Platform itself (configurable via `PLATFORM_NAMESPACE`) | Operator |
| `{slug}-dev` | Pipeline pods, agent pods | Pipeline executor, agent service |
| `{slug}-staging` | Staging deployments | Deployer reconciler |
| `{slug}-production` | Production deployments | Deployer reconciler |
| `{slug}-preview-{branch}` | Preview environments | Preview manager |
| `platform-test-{run_id}` | Ephemeral test infrastructure | Test harness |

Each namespace gets:
- Registry pull secret (if `PLATFORM_REGISTRY_URL` configured)
- Project secrets (scoped by environment)
- OTEL tokens (auto-injected for observability)
- `PLATFORM_API_TOKEN` (auto-injected for platform API access)

## Container Image

<!-- mermaid:diagrams/container-image.mmd -->
```mermaid
flowchart LR
    subgraph Build["Multi-stage Dockerfile"]
        stage1[rust:latest<br/>cargo build --release]
        stage2[gcr.io/distroless/cc<br/>Copy binary only]
    end

    subgraph Deploy["Deployment"]
        docker[just docker tag]
        kind_load[just deploy-local tag]
        kubectl[kubectl apply]
    end

    stage1 --> stage2 --> docker --> kind_load --> kubectl
```
<!-- /mermaid -->

The final image contains only:
- The compiled Rust binary
- The embedded Preact SPA (via rust-embed)
- CA certificates for TLS

## Production Target (Aspirational)

<!-- mermaid:diagrams/deployment-prod.mmd -->
```mermaid
flowchart TD
    subgraph Cluster["Production K8s Cluster"]
        subgraph Ingress["Ingress Layer"]
            traefik[Traefik IngressRoute]
            cert[cert-manager]
        end

        subgraph Platform["Namespace: platform"]
            deploy[Platform Deployment<br/>replicas: 1]
            pvc_git[PVC: git-repos]
            pvc_ops[PVC: ops-repos]
        end

        subgraph Infra["Infrastructure"]
            pg[PostgreSQL<br/>CNPG operator<br/>PVC: data]
            vk[Valkey<br/>PVC: data]
            minio[MinIO<br/>PVC: data]
        end

        subgraph Tenants["Per-Project Namespaces"]
            t1["{project}-staging"]
            t2["{project}-production"]
            t3["{project}-preview-*"]
        end
    end

    traefik --> deploy
    deploy --> pg
    deploy --> vk
    deploy --> minio
    deploy --> Tenants
```
<!-- /mermaid -->

### Production Requirements (Not Yet Implemented)

- Multi-replica support (requires leader election for background tasks)
- Persistent volumes for git repos and ops repos
- Backup strategy for PostgreSQL
- MinIO cluster mode or external S3
- TLS termination at ingress
- Network policies between tenant namespaces
