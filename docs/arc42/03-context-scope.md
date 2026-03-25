# 3. Context and Scope

## System Context (C4 Level 1)

The platform sits at the center, connecting developers, operators, and AI agents to infrastructure services.

<!-- mermaid:diagrams/context.mmd -->
```mermaid
C4Context
    title System Context — Platform

    Person(dev, "Developer", "Creates projects, pushes code, reviews MRs")
    Person(ops, "Operator", "Manages platform, monitors health")
    Person(agent, "AI Agent", "Autonomous code changes via Claude Code")

    System(platform, "Platform", "Unified AI-first DevOps platform — single Rust binary")

    System_Ext(claude, "Anthropic Claude API", "LLM provider for agent sessions")
    System_Ext(smtp, "SMTP Server", "Email notifications")
    SystemDb(pg, "PostgreSQL", "Primary database (64 migrations)")
    SystemDb(valkey, "Valkey", "Cache, pub/sub, rate limiting")
    SystemDb(minio, "MinIO", "Object storage (S3 API)")
    System_Ext(k8s, "Kubernetes API", "Pod orchestration for pipelines, agents, deploys")
    System_Ext(sandbox, "Agent Sandboxes", "Isolated K8s namespaces with NetworkPolicies per agent session")

    Rel(dev, platform, "Git push/pull, API, Web UI", "HTTPS / SSH")
    Rel(ops, platform, "Admin API, health checks", "HTTPS")
    Rel(agent, platform, "MCP servers, API tokens", "HTTPS")
    Rel(platform, claude, "Agent LLM calls", "HTTPS")
    Rel(platform, smtp, "Email delivery", "SMTP/TLS")
    Rel(platform, pg, "Queries (sqlx)", "TCP/TLS")
    Rel(platform, valkey, "Cache, pub/sub", "RESP protocol")
    Rel(platform, minio, "Artifacts, Parquet, LFS, OCI blobs", "S3 API")
    Rel(platform, k8s, "Pod CRUD, namespace mgmt", "HTTPS")
    Rel(platform, sandbox, "Spawns isolated agent pods", "K8s API")
    Rel(agent, sandbox, "Executes in sandbox", "ephemeral pod")
```
<!-- /mermaid -->

## External Interface Table

| Interface | Protocol | Direction | Purpose |
|---|---|---|---|
| **PostgreSQL** | TCP (sqlx) | Bidirectional | All persistent state — users, projects, pipelines, deploys, observability |
| **Valkey** | RESP (fred) | Bidirectional | Permission cache (5min TTL), rate limiting, pub/sub events, agent session state |
| **MinIO** | S3 API (opendal) | Bidirectional | Build artifacts, Parquet cold storage, Git LFS objects, OCI registry blobs |
| **Kubernetes** | HTTPS (kube-rs) | Outbound | Spawn pipeline pods, agent pods, deployment manifests, namespace management |
| **Anthropic Claude API** | HTTPS | Outbound | LLM inference for agent sessions (via Claude CLI in pods) |
| **SMTP** | SMTP/TLS (lettre) | Outbound | Email notifications |
| **Git clients** | Smart HTTP / SSH | Inbound | `git push`, `git pull`, `git clone` |
| **OTLP producers** | HTTP/protobuf | Inbound | Traces, logs, metrics from instrumented services |
| **Web browsers** | HTTPS | Inbound | Preact SPA (embedded via rust-embed) |
| **MCP clients** | HTTPS (JSON) | Inbound | 7 MCP servers for agent tool integration |

## Configuration (Key Env Vars)

| Variable | Default | Controls |
|---|---|---|
| `DATABASE_URL` | `postgres://platform:dev@localhost:5432/platform_dev` | PostgreSQL connection |
| `VALKEY_URL` | `redis://localhost:6379` | Valkey connection |
| `MINIO_ENDPOINT` | `http://localhost:9000` | S3 endpoint |
| `PLATFORM_LISTEN` | `0.0.0.0:8080` | HTTP bind address |
| `PLATFORM_MASTER_KEY` | — (required in prod) | AES-256-GCM key for secrets engine |
| `PLATFORM_NAMESPACE` | `platform` | K8s namespace for the platform itself |
| `PLATFORM_API_URL` | `http://platform.platform.svc.cluster.local:8080` | Cluster-internal URL for pods |
| `PLATFORM_CORS_ORIGINS` | (empty = deny) | Allowed CORS origins |
| `PLATFORM_DEV` | `false` | Dev mode (relaxed defaults) |

Full list: 87 config fields in `src/config.rs`.
