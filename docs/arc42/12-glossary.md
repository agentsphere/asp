# 12. Glossary

## Domain Terms

| Term | Definition |
|---|---|
| **Agent** | An AI user (Claude Code) that autonomously performs development tasks within a session |
| **Agent Session** | A time-bounded execution of an agent pod in K8s, with ephemeral identity and delegated permissions |
| **Canary Deployment** | Progressive rollout strategy where traffic is gradually shifted from stable to canary version |
| **Delegation** | Time-bounded permission grant from a human user to an agent user |
| **Deploy Target** | A named environment (staging, production, preview) for a project with a specific deployment strategy |
| **Deploy Release** | A specific version deployed to a target, tracked through the release phase state machine |
| **Feature Flag** | Runtime toggle (boolean, percentage, variant, JSON) controlling feature availability per environment |
| **Merge Request (MR)** | A request to merge a source branch into a target branch, with reviews and CI pipeline |
| **Ops Repo** | Separate git repository containing deployment manifests (Kustomize), managed by the deployer |
| **Pipeline** | A CI/CD execution triggered by push, MR, tag, or API; composed of ordered steps |
| **Pipeline Step** | A single unit of work within a pipeline (command, imagebuild, deploy_test, gitops_sync, deploy_watch) |
| **Preview Environment** | Ephemeral deployment per branch with TTL-based cleanup |
| **Project** | A git repository with associated issues, MRs, pipelines, deployments, and secrets |
| **Reconciler** | Background loop that continuously converges actual K8s state toward desired state in DB |
| **Release History** | Audit trail of deployment actions (created, promoted, rolled back, etc.) |
| **Rollout Analysis** | Automated evaluation of canary health using metric thresholds |
| **Secret** | An encrypted key-value pair scoped to a project/workspace/environment |
| **Slug** | K8s-safe DNS label derived from a name (alphanumeric + hyphens, 63 chars max) |
| **Workspace** | A grouping of projects with shared membership and implicit permissions |

## Technical Terms

| Term | Definition |
|---|---|
| **AppState** | Shared state struct passed to all handlers via `axum::extract::State`; contains all infrastructure clients |
| **AuthUser** | Axum extractor that resolves the authenticated user from Bearer token or session cookie |
| **Boundary** | Hard resource restriction on an API token (`boundary_project_id`, `boundary_workspace_id`) limiting visible resources |
| **EventBus** | Internal event dispatch system for cross-module communication |
| **fred** | Async Rust client for Valkey/Redis with connection pooling |
| **kube-rs** | Rust client library for the Kubernetes API |
| **Kustomize** | Kubernetes manifest templating tool used by the deployer for overlay-based configuration |
| **LFS** | Git Large File Storage — binary files stored in MinIO instead of git objects |
| **MCP Server** | Model Context Protocol server providing tool interfaces for AI agents |
| **minijinja** | Template engine used for rendering deployment manifests with values |
| **Notify** | `tokio::sync::Notify` primitive used to wake background tasks immediately on events |
| **OCI** | Open Container Initiative — standard for container images and distribution |
| **opendal** | Rust data access layer abstracting S3/GCS/Azure Blob behind a unified API |
| **OTLP** | OpenTelemetry Protocol — standard format for traces, logs, and metrics |
| **Parquet** | Columnar file format used for cold storage of observability data in MinIO |
| **prost** | Rust protobuf implementation used for OTLP message parsing |
| **rust-embed** | Compile-time embedding of static files (the Preact SPA) into the Rust binary |
| **russh** | Async Rust SSH library used for Git SSH protocol support |
| **Scope** | Permission strings on an API token (e.g., `["project:read"]`) that filter which permissions the token can exercise |
| **Server-Side Apply** | K8s API for declarative resource management with field ownership tracking |
| **sqlx** | Async Rust SQL toolkit with compile-time query validation |
| **Token Scope** | See **Scope** — distinct from **Boundary** which restricts resource visibility |
| **Valkey** | Open-source Redis fork (Linux Foundation); used for caching, pub/sub, rate limiting |
| **WebAuthn** | Web Authentication standard for passwordless login via passkeys |

## Abbreviations

| Abbreviation | Expansion |
|---|---|
| ADR | Architecture Decision Record |
| CNPG | CloudNativePG (PostgreSQL operator for Kubernetes) |
| CRUD | Create, Read, Update, Delete |
| E2E | End-to-End (test tier) |
| GCM | Galois/Counter Mode (AES encryption mode) |
| HMAC | Hash-based Message Authentication Code |
| IPC | Inter-Process Communication |
| LLM | Large Language Model |
| OTEL | OpenTelemetry |
| RBAC | Role-Based Access Control |
| RESP | Redis Serialization Protocol |
| SPA | Single Page Application |
| SSRF | Server-Side Request Forgery |
| TLS | Transport Layer Security |
| TTL | Time To Live |
