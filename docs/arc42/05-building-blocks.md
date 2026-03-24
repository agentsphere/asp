# 5. Building Block View

## Level 1 — Platform Modules

The platform is a single Rust binary (~68K LOC, plus ~38K LOC tests) composed of 15 modules that communicate through shared `AppState`. The database schema spans 64 migration pairs.

<!-- mermaid:diagrams/containers.mmd -->
```mermaid
C4Container
    title Building Block View — Level 1

    Enterprise_Boundary(core, "Core Services") {
        Container(api, "API Layer", "axum 0.8", "25+ HTTP sub-routers, auth extractors, validation")
        Container(auth, "Auth", "argon2 + WebAuthn", "Passwords, sessions, API tokens, passkeys")
        Container(rbac, "RBAC", "Valkey-cached", "Roles, permissions, delegation, resolution")
        Container(store, "Store", "AppState", "Postgres pool, Valkey pool, MinIO, K8s client, bootstrap")
    }

    Enterprise_Boundary(cicd, "CI/CD & Deployment") {
        Container(git, "Git Server", "russh + smart HTTP", "Push/pull, LFS, SSH, file browser, branch protection")
        Container(pipeline, "Pipeline Engine", "K8s pods", "YAML definition, step execution, triggers")
        Container(deployer, "Deployer", "Reconciler loop", "Ops repos, Kustomize rendering, canary, preview envs")
        Container(registry, "OCI Registry", "v2 API", "Image push/pull, tags, garbage collection")
    }

    Enterprise_Boundary(ai, "AI Agent Runtime") {
        Container(agent, "Agent Orchestrator", "K8s pods + CLI", "Session lifecycle, ephemeral identity, sandboxed execution")
    }

    Enterprise_Boundary(ops, "Operations & Observability") {
        Container(observe, "Observability", "OTLP + Parquet", "Ingest, flush, store, query, alerts")
        Container(secrets, "Secrets Engine", "AES-256-GCM", "Encrypt-at-rest, CRUD, LLM provider keys")
        Container(notify, "Notifications", "lettre + HMAC", "Email, webhooks, in-app dispatch")
    }
```
<!-- /mermaid -->

## Level 2 — Module Decomposition

### API Layer (`src/api/`)

The API module is the largest (~17K LOC, 30 files) with 25+ sub-routers merged into a single `Router<AppState>`:

| Sub-router | Purpose | Key Endpoints |
|---|---|---|
| `users` | Authentication | Login, logout, session management |
| `admin` | Administration | User CRUD, role assignment, delegation management |
| `projects` | Project management | CRUD, visibility, settings, namespace config |
| `issues` | Issue tracking | Create, list, update, comment |
| `merge_requests` | Code review | MR lifecycle, reviews, merge, auto-merge |
| `webhooks` | External integrations | CRUD, HMAC-signed delivery |
| `pipelines` | Build engine | Trigger, list, status, step logs |
| `deployments` | Deploy tracking | Status, promote, rollback, release history |
| `flags` | Feature flags | CRUD, rules, overrides, evaluation |
| `sessions` | Agent sessions | Create, list, messages, status |
| `secrets` | Secret management | CRUD, scoped injection |
| `notifications` | Notification queries | List, mark read |
| `passkeys` | WebAuthn | Registration, authentication |
| `user_keys` / `ssh_keys` / `gpg_keys` | Key management | SSH/GPG public keys |
| `workspaces` | Workspace management | CRUD, membership |
| `branch_protection` | Git policy | Protection rules |
| `releases` | Release management | Create, list, assets |
| `dashboard` | UI data | Aggregated dashboard views |
| `onboarding` | New user flow | Demo project creation |
| `setup` | Initial setup | First-admin via setup token |
| `cli_auth` | CLI auth | Claude CLI device-code flow |
| `commands` | Global commands | CRUD for platform commands |
| `downloads` | Agent binaries | Binary distribution |
| `health` | System health | Subsystem status |
| `llm_providers` | LLM config | Provider CRUD, validation |

Also merged: `git::browser_router()` for repository browsing (history, blame, tree, blob).

### Git Module (`src/git/`)

| Sub-module | Purpose |
|---|---|
| `smart_http` | Git Smart HTTP protocol (info/refs, upload-pack, receive-pack) |
| `ssh_server` | Git over SSH (russh) |
| `lfs` | Git LFS batch API + object storage via MinIO |
| `browser` | Repository browser (tree, blob, commits, branches, blame) |
| `hooks` | Post-receive hooks (trigger pipelines, update MR head_sha) |
| `repo` | Repository creation, bare repo management |
| `protection` | Branch protection rule enforcement |
| `signature` | GPG commit signature verification |
| `ssh_keys` / `gpg_keys` | User key management |
| `templates` | Template files for new repositories |

### Pipeline Engine (`src/pipeline/`)

<!-- mermaid:diagrams/components-pipeline.mmd -->
```mermaid
C4Component
    title Pipeline Engine — Components

    Component(def, "Definition", "src/pipeline/definition.rs", "Parse .platform.yaml, validate steps, images, commands")
    Component(trigger, "Trigger", "src/pipeline/trigger.rs", "on_push, on_mr, on_tag, on_api — create pipeline + steps")
    Component(executor, "Executor", "src/pipeline/executor.rs", "Background loop: claim pending, spawn K8s pods, poll status")
    Component(error, "Error", "src/pipeline/error.rs", "PipelineError enum with thiserror")

    Rel(trigger, def, "Parses definition at SHA")
    Rel(trigger, executor, "Wakes via pipeline_notify")
    Rel(executor, def, "Reads step config")
```
<!-- /mermaid -->

**Step types:**

| Step Type | Executor Behavior | Spawns Pod? |
|---|---|---|
| `command` | Run arbitrary container with setup commands | Yes |
| `imagebuild` | Generate kaniko command, inject secrets as `--build-arg`, push to registry | Yes (kaniko) |
| `deploy_test` | Create test namespace, apply testinfra manifests, spawn test pod, cleanup | Yes (test runner) |
| `gitops_sync` | Copy files to ops repo, merge variables, commit, publish OpsRepoUpdated event | No (in-process) |
| `deploy_watch` | Poll deploy_releases table until terminal phase | No (in-process) |

### Deployer (`src/deployer/`)

<!-- mermaid:diagrams/components-deployer.mmd -->
```mermaid
C4Component
    title Deployer — Components

    Component(recon, "Reconciler", "src/deployer/reconciler.rs", "Continuous loop: poll pending releases, apply manifests")
    Component(analysis, "Analysis", "src/deployer/analysis.rs", "Canary analysis: error rate, latency, custom metrics")
    Component(applier, "Applier", "src/deployer/applier.rs", "Server-side apply of K8s manifests")
    Component(renderer, "Renderer", "src/deployer/renderer.rs", "Render Kustomize/minijinja templates with values")
    Component(ops, "Ops Repo", "src/deployer/ops_repo.rs", "Clone/pull ops repos, read manifests, commit changes")
    Component(preview, "Preview", "src/deployer/preview.rs", "Ephemeral branch-scoped namespaces with TTL cleanup")
    Component(gateway, "Gateway", "src/deployer/gateway.rs", "Traffic routing for canary/AB deployments")
    Component(ns, "Namespace", "src/deployer/namespace.rs", "K8s namespace creation, secret injection, RBAC setup")
    Component(types, "Types", "src/deployer/types.rs", "ReleasePhase state machine, DeployTarget, TrackedResource")

    Rel(recon, applier, "Applies manifests")
    Rel(recon, renderer, "Renders templates")
    Rel(recon, ops, "Reads ops repo")
    Rel(recon, analysis, "Evaluates canary health")
    Rel(recon, ns, "Ensures namespace")
    Rel(recon, gateway, "Routes traffic")
    Rel(preview, ns, "Creates/deletes namespaces")
```
<!-- /mermaid -->

### Agent Orchestrator (`src/agent/`)

<!-- mermaid:diagrams/components-agent.mmd -->
```mermaid
C4Component
    title Agent Orchestrator — Components

    Component(svc, "Service", "src/agent/service.rs", "Session lifecycle: create, status, reaper")
    Component(id, "Identity", "src/agent/identity.rs", "Ephemeral agent users with role-based permissions")
    Component(prov, "Provider", "src/agent/provider.rs", "Provider interface, image resolution")
    Component(cli, "Claude CLI", "src/agent/claude_cli.rs", "Claude CLI integration and subprocess management")
    Component(invoke, "CLI Invoke", "src/agent/cli_invoke.rs", "CLI invocation and NDJSON streaming")
    Component(code, "Claude Code", "src/agent/claude_code.rs", "Claude Code protocol handling")
    Component(pubsub, "PubSub Bridge", "src/agent/pubsub_bridge.rs", "Valkey pub/sub for agent-platform communication")
    Component(create, "Create App", "src/agent/create_app.rs", "App scaffolding agent")
    Component(preview, "Preview Watcher", "src/agent/preview_watcher.rs", "Monitor preview endpoint availability")
    Component(acl, "Valkey ACL", "src/agent/valkey_acl.rs", "Per-session Valkey ACL configuration")
    Component(cmds, "Commands", "src/agent/commands.rs", "Agent command management")

    Rel(svc, id, "Creates ephemeral identity")
    Rel(svc, prov, "Resolves container image")
    Rel(svc, pubsub, "Subscribes before pod creation")
    Rel(svc, acl, "Configures session ACL")
    Rel(svc, cli, "Manages CLI subprocesses")
    Rel(cli, invoke, "Invokes Claude CLI")
    Rel(invoke, code, "Handles protocol")
    Rel(create, svc, "Creates agent session")
```
<!-- /mermaid -->

The most complex module (~12K LOC, 23 files, 14 sub-modules):

| Sub-module | Purpose |
|---|---|
| `service` | Session lifecycle: create, status, reaper |
| `identity` | Ephemeral agent users with role-based permissions |
| `provider` | Provider interface, image resolution (explicit > registry > default) |
| `claude_code` | Claude Code protocol handling |
| `claude_cli` | Claude CLI integration and subprocess management |
| `cli_invoke` | CLI invocation and NDJSON streaming |
| `pubsub_bridge` | Valkey pub/sub for agent ↔ platform communication |
| `create_app` | App scaffolding agent |
| `create_app_prompt` | LLM prompt templates for app creation |
| `commands` | Agent command management |
| `preview_watcher` | Monitor preview endpoint availability |
| `valkey_acl` | Per-session Valkey ACL configuration |
| `llm_validate` | LLM provider validation |
| `error` | AgentError enum |

**Agent role system:**

| Role | Scope | Permissions |
|---|---|---|
| `Dev` | Project | Code changes, branch management |
| `Ops` | Project | Deployment operations |
| `Test` | Project | Test execution |
| `Review` | Project | Code review |
| `Manager` | Workspace | Cross-project coordination |

### Observability (`src/observe/`)

| Sub-module | Purpose |
|---|---|
| `ingest` | OTLP HTTP endpoints (traces, logs, metrics) — protobuf parsing |
| `proto` | OTLP Protobuf type definitions (prost) |
| `parquet` | Time-based Parquet rotation to MinIO (cold storage) |
| `store` | Columnar query engine for Parquet files |
| `query` | Trace/log/metric query API with time-range filtering |
| `alert` | Alert rule evaluation loop, threshold checking, notification dispatch |
| `correlation` | Trace correlation (trace_id, session_id, project_id, user_id) |
| `tracing_layer` | Platform self-observability bridge |
| `error` | ObserveError enum |

**Background tasks** (5 spawned by `spawn_background_tasks()`):
1. Traces flush — channel drain → Postgres batch insert
2. Logs flush — channel drain → Postgres + Valkey pub/sub for live tail
3. Metrics flush — channel drain → Postgres upsert
4. Parquet rotation — time-based file rotation to MinIO
5. Alert evaluation — periodic rule evaluation against stored data

### Auth & RBAC (`src/auth/` + `src/rbac/`)

| Sub-module | Purpose |
|---|---|
| `auth/password` | Argon2 hashing, timing-safe verify, dummy hash for missing users |
| `auth/middleware` | `AuthUser` extractor (Bearer token → session cookie fallback) |
| `auth/token` | API token creation, validation, expiry |
| `auth/passkey` | WebAuthn registration and authentication |
| `auth/rate_limit` | Valkey-backed rate limiting (sliding window) |
| `auth/user_type` | `UserType` enum: human, agent, service_account |
| `auth/cli_creds` | CLI credential storage |
| `rbac/types` | `Permission` enum (all RBAC permissions) |
| `rbac/resolver` | Permission resolution with Valkey cache (5min TTL) |
| `rbac/delegation` | Time-bounded permission delegation |

### Store (`src/store/`)

Central infrastructure wiring:

| Sub-module | Purpose |
|---|---|
| `bootstrap` | DB initialization: system roles, permissions, first admin |
| `eventbus` | Event dispatch system for cross-module communication |
| `pool` | Postgres connection pool setup |
| `valkey` | Valkey connection pool setup |
| `commands_seed` | Seed built-in platform commands from .md files |

### OCI Registry (`src/registry/`)

Built-in container image registry (~3.4K LOC, 11 files) implementing the OCI Distribution Spec v2:

| Sub-module | Purpose |
|---|---|
| `v2` | OCI v2 API: manifests, blobs, tags, catalog |
| `gc` | Garbage collection of unreferenced layers |
| `proxy` | DaemonSet-based pull-through proxy for Kind clusters |

### Remaining Modules

| Module | LOC | Sub-modules | Purpose |
|---|---|---|---|
| `onboarding/` | ~2K | `demo_project`, `claude_auth`, `templates/` | Demo project scaffolding, Claude CLI auth flow |
| `secrets/` | ~1.7K | `engine`, `request`, `user_keys`, `llm_providers` | AES-256-GCM encryption, ephemeral secret requests, LLM provider keys |
| `health/` | ~800 | `checks` | Subsystem health checks (DB, Valkey, MinIO, K8s) |
| `notify/` | ~500 | `dispatch`, `email`, `webhook` | Route events to email (lettre SMTP) or webhooks (HMAC-SHA256) |
| `workspace/` | ~400 | `mod` | Workspace CRUD, membership, implicit project permissions |

## Module Communication

Modules communicate exclusively through `AppState` — there are no direct imports between module internals:

<!-- mermaid:diagrams/module-communication.mmd -->
```mermaid
flowchart TD
    subgraph AppState
        pool[PgPool]
        valkey[Valkey Pool]
        minio[MinIO Operator]
        kube[K8s Client]
        config[Config]
        webauthn[WebAuthn]
        pn[pipeline_notify]
        dn[deploy_notify]
    end

    api --> AppState
    git --> AppState
    pipeline --> AppState
    deployer --> AppState
    agent --> AppState
    observe --> AppState
    registry --> AppState
    secrets --> AppState
    notify --> AppState
    auth --> AppState
    rbac --> AppState
    onboarding --> AppState
    health --> AppState
    workspace --> AppState
    store --> AppState
```
<!-- /mermaid -->
