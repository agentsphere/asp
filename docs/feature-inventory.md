# Platform Feature & Functionality Inventory

## Context

Comprehensive inventory of every module in the platform — a single Rust binary (~72K LOC) replacing 8+ off-the-shelf services with a unified platform for code hosting, CI/CD, deployment, agent orchestration, and observability.

**15 modules** | **~72K LOC** | **64 migration pairs** | **~1600 unit tests** | **52 integration test files** | **9 E2E test files**

---

## Module 1: `auth` (8 files)

Identity, authentication, and session management.

| File | Purpose |
|---|---|
| `middleware.rs` | `AuthUser` extractor — checks Bearer token → `api_tokens`, then session cookie → `auth_sessions` |
| `password.rs` | Argon2id hashing, timing-safe verify, `dummy_hash()` for missing users |
| `token.rs` | API token generation (`plat_` prefix), SHA-256 hashed storage, expiry enforcement (1–365 days) |
| `passkey.rs` | WebAuthn/FIDO2 registration + authentication via `webauthn_rs` |
| `rate_limit.rs` | Valkey-backed sliding window rate limiter (`check_rate()`) |
| `user_type.rs` | `UserType` enum: Human vs Agent user distinction |
| `cli_creds.rs` | Ephemeral CLI credentials for agent sessions (short-lived tokens) |
| `mod.rs` | Re-exports |

**Key features**: Timing-safe login, secure cookie sessions, API tokens with project/workspace scoping, WebAuthn passkeys, rate limiting, agent identity

---

## Module 2: `rbac` (5 files)

Role-based access control with project-scoped permissions.

| File | Purpose |
|---|---|
| `types.rs` | 21 `Permission` variants (AdminUsers, ProjectRead/Write, RegistryPush/Pull, PipelineRun, etc.), `Role` struct |
| `resolver.rs` | Permission resolution with Valkey cache (5-min TTL, key `perms:{user_id}:{project_id}`), `has_permission()`, `invalidate_permissions()` |
| `delegation.rs` | Permission delegation — users can grant subsets of their permissions to others |
| `middleware.rs` | `require_permission()` route layer (needs `from_fn_with_state`) |
| `mod.rs` | Re-exports |

**Key features**: Hierarchical permission resolution (system → project), Valkey-cached lookups, delegation chains, cache invalidation on role changes

---

## Module 3: `api` (30 files)

HTTP API layer — 100+ endpoints across 22 sub-routers.

| File | Endpoints | Purpose |
|---|---|---|
| `projects.rs` | CRUD + settings | Project lifecycle, soft-delete, visibility |
| `issues.rs` | CRUD + comments | Project-scoped issue tracker with auto-incrementing numbers |
| `merge_requests.rs` | CRUD + reviews + merge | MRs with review workflow, `--no-ff` merge via git worktree |
| `webhooks.rs` | CRUD + `fire_webhooks()` | HMAC-SHA256 signed webhook delivery, SSRF protection |
| `pipelines.rs` | CRUD + triggers | Pipeline run management, status transitions |
| `deployments.rs` | Status + logs | Deployment tracking |
| `sessions.rs` | CRUD + lifecycle | Agent session management (create/list/stop/stream) |
| `secrets.rs` | CRUD + requests | Secret management with agent request flow |
| `notifications.rs` | List + mark read | In-app notification queries |
| `passkeys.rs` | Register + auth | WebAuthn ceremony endpoints |
| `admin.rs` | Users + roles + delegations | Admin CRUD with audit logging |
| `users.rs` | Profile + password | User self-service |
| `workspaces.rs` | CRUD + members | Workspace management |
| `user_keys.rs` | SSH key CRUD | User SSH public key management |
| `gpg_keys.rs` | GPG key CRUD | GPG signature key management |
| `ssh_keys.rs` | SSH key listing | SSH key list endpoints |
| `cli_auth.rs` | Device flow | CLI authentication flow for agent-runner |
| `commands.rs` | Agent commands | Send commands to running agent sessions |
| `downloads.rs` | Binary downloads | Agent-runner binary download endpoint |
| `dashboard.rs` | Stats | Dashboard summary statistics |
| `setup.rs` | Initial setup | First-boot admin creation with setup token |
| `helpers.rs` | Utilities | `get_json`, `post_json`, pagination helpers, `ListParams`/`ListResponse` |
| `mod.rs` | Router composition | Merges all sub-routers |

**Key features**: RESTful JSON API, `AuthUser` on all endpoints, RBAC checks (inline or helper), audit logging on mutations, webhook dispatch after events, pagination (limit/offset), input validation

---

## Module 4: `store` (6 files)

Shared state and infrastructure connections.

| File | Purpose |
|---|---|
| `mod.rs` | `AppState` struct (pool, valkey, minio, kube, config, webauthn, pipeline_notify, deploy_notify, secret_requests, cli_sessions) |
| `pool.rs` | Postgres connection pool (`sqlx::PgPool`) with migration runner |
| `valkey.rs` | Valkey (Redis-compatible) connection pool via `fred` |
| `bootstrap.rs` | First-boot initialization: system roles, permissions, admin user (dev) or setup token (prod), `platform-runner` project + registry repo |
| `eventbus.rs` | Valkey pub/sub event bus for real-time WebSocket notifications |

**Key features**: Single `AppState` shared across all handlers, auto-migration on startup, bootstrap idempotency, real-time event bus

---

## Module 5: `config` (1 file)

40+ environment variable configuration fields.

| Category | Fields |
|---|---|
| **Core** | `listen`, `database_url`, `valkey_url`, `minio_*`, `dev_mode` |
| **Paths** | `git_repos_path`, `ops_repos_path`, `seed_images_path` |
| **Auth** | `admin_password`, `secure_cookies`, `trust_proxy`, `permission_cache_ttl_secs` |
| **WebAuthn** | `webauthn_rp_id`, `webauthn_rp_origin`, `webauthn_rp_name` |
| **K8s** | `namespace`, `pipeline_namespace`, `agent_namespace` |
| **Registry** | `registry_url`, `registry_node_url` |
| **SMTP** | `smtp_host`, `smtp_port`, `smtp_from`, `smtp_username`, `smtp_password` |
| **CORS** | `cors_origins` |
| **Agent** | `api_url`, `claude_api_key`, `max_cli_subprocesses` |
| **SSH** | `ssh_listen`, `ssh_host_key_path` |
| **Secrets** | `master_key` |

**Key features**: Env var loading with sensible defaults, dev-mode fallbacks, `test_default()` for tests

---

## Module 6: `git` (12 files)

Git server — smart HTTP protocol, SSH, LFS, repository browser.

| File | Purpose |
|---|---|
| `smart_http.rs` | Git smart HTTP protocol: `info/refs`, `git-upload-pack`, `git-receive-pack` |
| `ssh_server.rs` | SSH git server with public key auth (russh) |
| `repo.rs` | Bare repository initialization with templates |
| `lfs.rs` | Git LFS batch API with MinIO presigned URLs |
| `browser.rs` | Repository browser API (tree, blob, commits, diff) |
| `hooks.rs` | Post-receive hook processing — triggers pipelines on push |
| `templates.rs` | 6 template files for new projects (`.platform.yaml`, `Dockerfile`, `Dockerfile.dev`, deploy manifest, `CLAUDE.md`, `README.md`) |
| `mod.rs` | Router composition + `git_protocol_router()` |

**HTTP routes**:
- `GET /{owner}/{repo}/info/refs` — Ref advertisement
- `POST /{owner}/{repo}/git-upload-pack` — Clone/fetch (bidirectional streaming)
- `POST /{owner}/{repo}/git-receive-pack` — Push
- `POST /{owner}/{repo}/info/lfs/objects/batch` — LFS batch API
- Browser: tree, blob, commits, diff endpoints

**Key features**: Smart HTTP + SSH dual transport, LFS with MinIO presigned URLs, post-receive pipeline triggers, project templates, visibility-aware access (public repos allow anonymous reads), 404 on denied access

---

## Module 7: `pipeline` (5 files)

CI/CD build engine — YAML-defined pipelines executed as K8s pods.

| File | Purpose |
|---|---|
| `definition.rs` | `.platform.yaml` parser — validates steps, images, commands, container image injection checks |
| `executor.rs` | Background task: spawns K8s pods per pipeline step, logs streaming, status transitions |
| `trigger.rs` | `on_push()` — triggers pipeline runs when git refs are pushed |
| `error.rs` | `PipelineError` enum |
| `mod.rs` | `PipelineStatus` state machine (Pending → Running → Success/Failure/Cancelled), `slugify_branch()` |

**Background task**: `executor::run()` — wakes on `pipeline_notify`, polls pending runs, creates K8s pods, streams logs, updates status

**Key features**: YAML pipeline definition, K8s pod execution per step, container image validation, Kaniko image building, registry push, branch-based triggers, status state machine, log streaming, per-project namespaces (`{slug}-dev`)

---

## Module 8: `deployer` (11 files)

Continuous deployment — GitOps reconciliation with preview environments.

| File | Purpose |
|---|---|
| `reconciler.rs` | Background task: continuous reconciliation of desired vs actual K8s state |
| `applier.rs` | K8s server-side apply (kubectl equivalent) with `kind_to_plural()` mapping |
| `renderer.rs` | Kustomize overlay rendering |
| `ops_repo.rs` | Operations repo management (Kustomize/Helm manifests) |
| `namespace.rs` | Per-project namespace creation with `NetworkPolicy` isolation |
| `preview.rs` | Background task: ephemeral preview environments per branch, TTL-based cleanup |
| `error.rs` | `DeployerError` enum |
| `mod.rs` | Re-exports |

**Background tasks**: `reconciler::run()` (continuous), `preview::run()` (TTL cleanup)

**Key features**: GitOps reconciliation, K8s server-side apply, Kustomize rendering, per-project namespaces with NetworkPolicy, preview environments with branch-based slugs, TTL-based cleanup, deploy_notify wakeup

---

## Module 9: `agent` (23 files)

AI agent orchestration — ephemeral Claude sessions in K8s pods.

| Submodule | Files | Purpose |
|---|---|---|
| `service.rs` | 1 | Session lifecycle: create, list, stop, reap stale sessions |
| `identity.rs` | 1 | Ephemeral agent identity with scoped Valkey ACL + API tokens |
| `provider.rs` | 1 | Provider config: image resolution (explicit → registry → default) |
| `valkey_acl.rs` | 1 | Valkey ACL user creation/teardown per session |
| `pubsub_bridge.rs` | 1 | Valkey pub/sub bridge: agent pod ↔ WebSocket client |
| `commands.rs` | 1 | Command dispatch to running agents |
| `create_app.rs` | 1 | "Create app" workflow: agent-guided project scaffolding |
| `create_app_prompt.rs` | 1 | System prompt for create-app agent |
| `cli_invoke.rs` | 1 | CLI subprocess invocation for agent tasks |
| `claude_code/` | 4 | Pod spec builder, adapter, progress parsing |
| `claude_cli/` | 6 | CLI subprocess transport: session management, message protocol, control commands |
| `error.rs` | 1 | `AgentError` enum |
| `mod.rs` | 1 | Re-exports |

**Background task**: `service::run_reaper()` — cleans up stale/terminated agent sessions

**Key features**: Ephemeral K8s pod per session, scoped Valkey ACL per agent, pub/sub real-time communication, CLI subprocess transport, agent-runner binary in pods, pull secret injection, session reaping, create-app workflow, progress streaming

---

## Module 10: `observe` (9 files)

Observability — OTLP ingest, Parquet storage, query engine, alerting.

| File | Purpose |
|---|---|
| `ingest.rs` | HTTP endpoints for OTLP traces, logs, metrics (protobuf) + channel-based buffering |
| `proto.rs` | Protobuf type definitions for OTLP wire format |
| `parquet.rs` | Time-based Parquet file rotation to MinIO |
| `store.rs` | Columnar query engine over Parquet files |
| `query.rs` | Query API endpoints: traces, logs, metrics with time-range filtering |
| `alert.rs` | Background alert evaluation against stored data, notification dispatch |
| `correlation.rs` | Trace-to-log correlation via trace_id/span_id |
| `error.rs` | `ObserveError` enum |
| `mod.rs` | `spawn_background_tasks()` — launches 5 tasks |

**Background tasks** (5): Traces flush, logs flush, metrics flush, Parquet rotation, alert evaluation

**Key features**: OTLP-compatible ingest (traces + logs + metrics), Parquet columnar storage on MinIO, time-range query API, alert rules evaluation, trace-log correlation, buffered channel ingestion

---

## Module 11: `registry` (11 files)

OCI Distribution Spec v2 compliant image registry.

| File | Purpose |
|---|---|
| `auth.rs` | `RegistryUser` extractor — Bearer + Basic auth with OCI-compliant 401 |
| `blobs.rs` | Blob operations: HEAD, GET, POST (start upload), PATCH (chunk), PUT (complete) |
| `manifests.rs` | Manifest operations: HEAD, GET, PUT, DELETE with blob link verification |
| `tags.rs` | Tag listing endpoint |
| `types.rs` | OCI manifest/descriptor types, media type constants |
| `digest.rs` | SHA-256 digest parsing and validation |
| `pull_secret.rs` | K8s `imagePullSecret` generation for pods |
| `seed.rs` | OCI layout tarball parser — seeds registry from filesystem tarballs on startup |
| `gc.rs` | Garbage collection — cleanup unreferenced blobs |
| `error.rs` | OCI-compliant error responses (NAME_UNKNOWN, BLOB_UNKNOWN, etc.) |
| `mod.rs` | Router + `resolve_repo_with_access()` (ownership + RBAC) |

**HTTP routes** (OCI Distribution Spec):
- `GET /v2/` — Version check
- `HEAD/GET /v2/{name}/blobs/{digest}` — Blob lookup/download
- `POST /v2/{name}/blobs/uploads/` — Start upload
- `PATCH /v2/{name}/blobs/uploads/{uuid}` — Chunked upload
- `PUT /v2/{name}/blobs/uploads/{uuid}` — Complete upload
- `HEAD/GET/PUT/DELETE /v2/{name}/manifests/{reference}` — Manifest CRUD
- `GET /v2/{name}/tags/list` — Tag listing

**Background task**: `gc::run()` — garbage collection of unreferenced blobs

**Key features**: Full OCI Distribution Spec v2, chunked blob upload via Valkey sessions, MinIO blob storage, lazy repo creation on first push, image index (multi-arch) support, digest verification, registry seeding from OCI tarballs, pull secret generation, GC

---

## Supporting Modules

### `workspace` (3 files)
Workspace hierarchy: workspace → projects. Roles: owner/admin/member. Implicit registry pull for members.

### `secrets` (5 files)
AES-256-GCM encryption at rest with `PLATFORM_MASTER_KEY`. Hierarchy: workspace → project → environment. Ephemeral in-memory secret requests for agent sessions (5-min TTL).

### `notify` (4 files)
Notification dispatch to 3 channels: InApp (DB), Email (SMTP via lettre), Webhook. Rate-limited (100/user/hour). Header injection protection. Graceful degradation when SMTP unconfigured.

### `validation` (1 file)
12+ check functions: `check_name`, `check_email`, `check_length`, `check_branch_name`, `check_labels`, `check_url`, `check_lfs_oid`, etc.

### `error` (1 file)
`ApiError` enum mapping domain errors to HTTP status codes. Consistent JSON error responses.

### `audit` (1 file)
`AuditEntry` struct for `audit_log` table. All mutations write audit records with actor, action, resource, IP.

### `ui` (1 file)
Preact SPA served via `rust-embed`. SPA-aware fallback to `index.html`. Cache headers: `no-cache` for HTML, 1-day for assets.

---

## Background Tasks Summary

| Task | Module | Wakeup |
|---|---|---|
| Pipeline executor | `pipeline` | `pipeline_notify` |
| Event bus | `store` | Valkey pub/sub |
| Deployer reconciler | `deployer` | `deploy_notify` |
| Preview cleanup | `deployer` | Timer (TTL) |
| Agent reaper | `agent` | Timer |
| Traces flush | `observe` | Channel buffer |
| Logs flush | `observe` | Channel buffer |
| Metrics flush | `observe` | Channel buffer |
| Parquet rotation | `observe` | Timer |
| Alert evaluation | `observe` | Timer |
| Registry GC | `registry` | Timer |
| SSH server | `git` | Listener (optional) |
| Session cleanup | `main` | Hourly timer |

---

## Infrastructure & External Dependencies

| Service | Library | Purpose |
|---|---|---|
| PostgreSQL | `sqlx` (compile-time checked) | Primary data store, 64 migration pairs |
| Valkey (Redis) | `fred` | Cache, rate limiting, pub/sub, upload sessions, ACL |
| MinIO (S3) | `opendal` | Blob storage (registry, LFS, Parquet, artifacts) |
| Kubernetes | `kube-rs` | Pod orchestration (pipelines, agents, deployments) |
| SMTP | `lettre` | Email notifications |

---

## Test Coverage

| Tier | Count | Runtime | Infrastructure |
|---|---|---|---|
| Unit | ~1600 | ~1s | None |
| Integration | 52 files | ~2.5 min | dev cluster (Postgres, Valkey, MinIO, K8s) |
| E2E | 9 files | ~2.5 min | dev cluster |
| FE-BE | 33+ | ~30s | dev cluster |
