# Platform Architecture

Condensed from 25 implementation plans (available in git history). For coding conventions, see `CLAUDE.md`. For the full schema and design rationale, see `plans/unified-platform.md`.

## Overview

Single Rust binary (~23K LOC) + Preact SPA replacing Gitea, Woodpecker, Authelia, OpenObserve, Maddy, and OpenBao. Primary users: AI agents (Claude Code). Humans are auditors/monitors.

**Kept as infrastructure**: PostgreSQL (CNPG), Valkey, MinIO, Traefik, OTel Collector.

## Module Map

```
src/
├── auth/          Password hashing (argon2), sessions, API tokens, passkeys (WebAuthn), rate limiting
├── rbac/          Roles, permissions, time-bounded delegation, Valkey-cached resolution
├── api/           14 HTTP handler modules (axum), all wired via .merge() in mod.rs
├── git/           Smart HTTP (push/pull), LFS → MinIO, file browser, post-receive hooks
├── pipeline/      .platform.yaml parsing, K8s pod execution per step, log streaming
├── deployer/      Reconciler loop, ops repo management, Kustomize rendering, K8s applier, preview envs
├── agent/         Session lifecycle, ephemeral identity with delegated perms, Claude Code provider
├── observe/       OTLP ingest (prost protobuf), Parquet → MinIO, query API, alert evaluation
├── secrets/       AES-256-GCM encryption engine, CRUD
├── notify/        Email (lettre SMTP), webhooks (HMAC-SHA256), in-app notifications
└── store/         PgPool, Valkey Pool, MinIO Operator, K8s client, bootstrap
```

Additional:
- `ui/src/` — Preact SPA (esbuild, rust-embed): Dashboard, Projects, Issues, MRs, Pipelines, Sessions, Observe, Admin
- `mcp/servers/` — 6 MCP servers (Node.js) for agent integration: core, admin, issues, pipeline, deploy, observe

## Data Flow

```
git push → post-receive hook → pipeline trigger → K8s pod per step → artifacts to MinIO
                                                                    → deployment row written

deployer reconciler (background) → reads deployments table → pulls ops repo
                                 → renders Kustomize → kubectl apply → updates status

agent session → ephemeral identity + delegated perms → K8s pod (Claude Code)
             → commits to branch → can trigger pipeline

OTLP ingest → Postgres (hot, 48h) + MinIO Parquet (cold, 90d+)
           → alert evaluation loop → notifications
```

## Key Design Decisions

1. **Single binary** — one K8s Deployment, one IngressRoute. Eliminates cross-service integration.
2. **Users and agents share identity model** — agents are users with the `agent` type. RBAC applies uniformly. Delegation lets humans grant scoped, time-bounded permissions.
3. **Build != Deploy** — pipelines produce artifacts and write desired state to `deployments` table. The deployer reconciler applies manifests from ops repos. Rollback = update image_ref, deployer handles the rest.
4. **Postgres as the brain** — unified schema (24 migrations). sqlx compile-time query validation.
5. **Type-safe state machines** — all status fields are Rust enums with `can_transition_to()`. Invalid transitions caught at compile time.
6. **Structured telemetry** — every log/span/metric carries correlation envelope (trace_id, session_id, project_id, user_id).

## Database Schema Overview

24 migration pairs. Core tables:

| Domain | Tables |
|--------|--------|
| Identity & RBAC | users, roles, permissions, role_permissions, user_roles, delegations, auth_sessions, api_tokens, passkey_credentials |
| Projects | projects, issues, comments, webhooks, merge_requests, mr_reviews |
| Agents | agent_sessions, agent_messages |
| Pipelines | pipelines, pipeline_steps, artifacts |
| Deploy | ops_repos, deployments, deployment_history, preview_deployments |
| Observability | traces, spans, log_entries, metric_series, metric_samples, alert_rules, alert_events |
| Secrets | secrets |
| Notifications | notifications |
| Audit | audit_log |

Full DDL in `plans/unified-platform.md`.

## RBAC Model

- **System roles**: admin (all), developer, ops, agent (none by default — via delegation), viewer
- **Permission resolution**: `global_role_perms ∪ project_role_perms ∪ active_delegations`
- **Cached**: Valkey per `(user_id, project_id)` with configurable TTL (default 300s)
- **Token scopes**: API tokens can be scoped to specific permissions; `scope_allows()` intersects
- **User types**: human, agent, service_account — affects login capability and permission grants

## Background Tasks

The binary spawns several background tokio tasks:

| Task | Module | Interval |
|------|--------|----------|
| Pipeline executor | pipeline | Event-driven (Notify) |
| Deployer reconciler | deployer | Continuous loop |
| Preview cleanup | deployer | 15s |
| Traces flush | observe | Periodic |
| Logs flush | observe | Periodic |
| Metrics flush | observe | Periodic |
| Parquet rotation | observe | Time-based |
| Alert evaluation | observe | Periodic |
| Session cleanup | auth | Hourly |

## Security Layers

- **Input validation**: `src/validation.rs` — all user inputs validated at handler boundary
- **Rate limiting**: Valkey-backed, applied to login and auth endpoints
- **SSRF protection**: `validate_webhook_url()` blocks private IPs, metadata endpoints
- **Container image validation**: `check_container_image()` blocks shell injection
- **Timing-safe auth**: always run argon2 verify (dummy hash for missing users)
- **Body size limits**: 10 MB API, 500 MB Git push/LFS
- **Security headers**: X-Frame-Options, X-Content-Type-Options, Referrer-Policy
- **Audit trail**: all mutations write to `audit_log` with actor, action, resource, IP

## Testing Pyramid

| Layer | Count | Location | Run with |
|-------|-------|----------|----------|
| Unit | 716 | `src/**/*.rs` inline `#[cfg(test)]` | `just test-unit` |
| Integration | 574 | `tests/*_integration.rs` (25 files) | `just test-integration` |
| E2E | 49 | `tests/e2e_*.rs` (5 files, ignored) | `just test-e2e` |

Both integration and E2E tests use `hack/test-in-cluster.sh` to deploy ephemeral Postgres, Valkey, and MinIO pods in isolated cluster namespaces per run. `#[sqlx::test]` provides per-test DB isolation. Requires dev cluster (`just cluster-up`).

## Ops Repo Pattern

Deployments use separate ops repos for manifest management:

```
ops-repo/
├── apps/
│   └── my-app/
│       ├── deployment.yaml    # template with {{ .ImageRef }}
│       ├── service.yaml
│       └── values.yaml        # defaults, overridden by deployments.values_override
└── platform/
    └── ...
```

The deployer clones/pulls ops repos, renders manifests with Kustomize, and applies via kube-rs.

## Preview Environments

Branch-scoped ephemeral deployments:
- `slugify_branch()` converts branch names to K8s-safe DNS labels (63 chars max)
- Each preview gets its own namespace, Deployment, and Service
- TTL-based cleanup removes expired previews
- Auto-stopped when MR is merged
