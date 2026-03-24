# 6. Runtime View

8 scenarios covering the platform's major flows. For the CI/CD scenarios (R1-R4), see also [`plans/cicd-process-spec-v2.md`](../../plans/cicd-process-spec-v2.md).

## R1: Full CI/CD Lifecycle (Overview)

The end-to-end flow from code push through production deployment:

<!-- mermaid:diagrams/runtime-cicd-overview.mmd -->
```mermaid
flowchart LR
    subgraph CodeRepo["Code Repo"]
        push[git push]
        mr[MR Created]
        yaml[.platform.yaml]
    end

    subgraph MRPipeline["MR Pipeline"]
        build_mr[imagebuild x4]
        test[deploy_test]
    end

    subgraph MergeGate["Merge Gate"]
        auto[auto_merge check]
        merge["--no-ff merge"]
    end

    subgraph MainPipeline["Main Pipeline"]
        build_main[imagebuild x3]
        sync[gitops_sync]
        watch[deploy_watch]
    end

    subgraph OpsRepo["Ops Repo"]
        copy[Copy deploy/]
        values[Merge variables]
        commit[Commit to staging]
    end

    subgraph Staging["Staging Deploy"]
        recon_s[Reconciler]
        ns_s[Create namespace]
        apply_s[Apply manifests]
        canary_s[Canary analysis]
        promote_s[Promote 100%]
    end

    subgraph Production["Production Deploy"]
        manual[Manual promote]
        recon_p[Reconciler]
        canary_p[Canary analysis]
        promote_p[Promote 100%]
    end

    push --> mr --> yaml --> build_mr --> test --> auto
    auto --> merge --> build_main --> sync --> watch
    sync --> copy --> values --> commit
    commit --> recon_s --> ns_s --> apply_s --> canary_s --> promote_s
    promote_s --> manual --> recon_p --> canary_p --> promote_p
```
<!-- /mermaid -->

---

## R2: MR Pipeline — Trigger, Build, Test

Covers the flow from feature branch push through pipeline execution.

<!-- mermaid:diagrams/runtime-mr-pipeline.mmd -->
```mermaid
sequenceDiagram
    participant Dev as Developer
    participant Git as Git Server
    participant Hook as Post-Receive Hook
    participant EB as EventBus
    participant Trig as pipeline::trigger
    participant DB as PostgreSQL
    participant Exec as Pipeline Executor
    participant K8s as Kubernetes
    participant Reg as OCI Registry
    participant S3 as MinIO

    Dev->>Git: git push (feature branch)
    Git->>Hook: post-receive
    Hook->>EB: PushEvent(branch, sha)
    EB->>Trig: on_mr()

    Note over Trig: Read .platform.yaml at SHA
    Trig->>Trig: Parse + validate definition
    Trig->>Trig: Match trigger (mr.actions: [opened])
    Trig->>DB: INSERT pipelines (status=pending)
    Trig->>DB: INSERT pipeline_steps (per step)
    Trig-->>Exec: pipeline_notify.notify_one()

    Exec->>DB: Claim pipeline (status → running)
    Exec->>K8s: Ensure project namespace

    loop For each step (DAG order)
        alt imagebuild step
            Exec->>K8s: Create Pod (init: git clone, main: kaniko)
            Exec->>K8s: Inject secrets as --build-arg
            K8s->>Reg: Push built image
        else deploy_test step
            Exec->>K8s: Create test namespace
            Exec->>K8s: Apply testinfra manifests
            Exec->>K8s: Spawn test runner pod
            K8s-->>Exec: Test results
            Exec->>K8s: Cleanup test namespace
        else command step
            Exec->>K8s: Spawn pod with user image
        end
        Exec->>DB: Update step status
        Exec->>S3: Store step logs
    end

    Exec->>DB: finalize_pipeline(success/failure)
    Exec->>EB: fire_webhooks("build", result)
    Exec->>Trig: try_auto_merge() if success
```
<!-- /mermaid -->

---

## R3: Auto-Merge, Main Pipeline, GitOps Sync

After a successful MR pipeline, the auto-merge and main branch pipeline flow:

<!-- mermaid:diagrams/runtime-merge-gitops.mmd -->
```mermaid
sequenceDiagram
    participant Exec as Pipeline Executor
    participant Git as Git Server
    participant DB as PostgreSQL
    participant Ops as Ops Repo
    participant EB as EventBus

    Note over Exec: MR pipeline succeeded
    Exec->>DB: Check auto_merge=true on MR
    Exec->>Git: do_merge() (git worktree --no-ff)
    Git->>Exec: merge_sha

    Exec->>DB: on_push(main, merge_sha)
    Exec->>DB: Parse .platform.yaml
    Exec->>DB: INSERT pipeline (trigger=push)
    Exec-->>Exec: pipeline_notify.notify_one()

    Note over Exec: Main pipeline steps
    par Build images
        Exec->>Exec: build-app (imagebuild)
        Exec->>Exec: build-canary (imagebuild)
        Exec->>Exec: build-dev (imagebuild)
    end

    Note over Exec: gitops_sync step (in-process)
    Exec->>Ops: Look up ops repo for project
    Exec->>Ops: Copy deploy/ + .platform.yaml
    Exec->>Ops: Merge variables_staging.yaml → values/staging.yaml
    Exec->>Ops: Build values JSON (image_ref, canary_image_ref)
    Exec->>Ops: Commit to ops repo staging branch
    Exec->>EB: Publish OpsRepoUpdated(staging)
    Exec->>DB: Register feature flags + prune old

    Note over Exec: deploy_watch step (in-process)
    loop Poll every 5s
        Exec->>DB: Check deploy_releases for staging
        alt Phase is terminal
            Exec->>Exec: Write result to step log
        end
    end
```
<!-- /mermaid -->

---

## R4: Deploy Pipeline — Reconciler + Canary Progression

The deployment reconciliation flow triggered by ops repo updates:

<!-- mermaid:diagrams/runtime-deploy-canary.mmd -->
```mermaid
sequenceDiagram
    participant EB as EventBus
    participant Handler as OpsRepo Handler
    participant DB as PostgreSQL
    participant Recon as Reconciler
    participant K8s as Kubernetes
    participant Secrets as Secrets Engine
    participant Renderer as Renderer
    participant Analysis as Analysis Loop

    EB->>Handler: OpsRepoUpdated(environment)
    Handler->>Handler: Read .platform.yaml from ops repo
    Handler->>Handler: Extract strategy + rollout_config
    Handler->>DB: Upsert deploy_targets
    Handler->>DB: INSERT deploy_releases (phase=pending)
    Handler-->>Recon: deploy_notify.notify_one()

    Recon->>DB: Claim release (phase → progressing)

    Note over Recon: handle_pending
    Recon->>K8s: Create namespace {slug}-{env}
    Recon->>Secrets: inject_project_secrets()
    Note over Secrets: Query secrets (scope matching env)<br/>Auto-inject OTEL tokens<br/>Auto-inject PLATFORM_API_TOKEN<br/>Create K8s Secret
    Recon->>K8s: ensure_registry_pull_secret()
    Recon->>Renderer: render_manifests(minijinja + values)
    Recon->>K8s: apply_manifests(server-side apply)

    alt Strategy = canary
        Recon->>K8s: Set initial traffic weight (10%)
        Recon->>DB: phase → progressing

        loop Analysis loop (every 15s)
            Analysis->>Analysis: Check rollback triggers
            Analysis->>Analysis: Evaluate gates (error_rate < 0.05)
            alt Pass
                Analysis->>K8s: Advance weight (10→25→50→100%)
                Analysis->>DB: Record release_history
            else Fail
                Analysis->>DB: phase → rolling_back
                Analysis->>K8s: Route 100% to stable
            end
        end

        Note over Recon: handle_promoting
        Recon->>K8s: Route 100% to canary (now stable)
        Recon->>Renderer: Re-render manifests
        Recon->>DB: phase=completed, health=healthy

    else Strategy = rolling
        Recon->>K8s: Apply manifests directly
        Recon->>DB: phase=completed
    end
```
<!-- /mermaid -->

### Manual Promotion (Staging → Production)

```
API → POST /promote-staging
API → Merge ops repo staging branch → main branch
API → Publish OpsRepoUpdated(production)
→ Same reconciler flow for production environment
```

---

## R5: Authentication Flow

Login → session → AuthUser extractor → RBAC permission check:

<!-- mermaid:diagrams/runtime-auth.mmd -->
```mermaid
sequenceDiagram
    participant Client
    participant API as API Handler
    participant MW as AuthUser Extractor
    participant DB as PostgreSQL
    participant VK as Valkey
    participant RBAC as Permission Resolver

    Client->>API: POST /api/auth/login {name, password}
    API->>VK: check_rate("login", identifier, 10, 300s)
    API->>DB: SELECT user by name
    API->>API: argon2::verify (timing-safe, dummy hash if missing)
    API->>DB: INSERT auth_sessions (token_hash, expires_at)
    API-->>Client: Set-Cookie: session=token

    Note over Client,API: Subsequent request
    Client->>MW: GET /api/projects (Cookie or Bearer)

    MW->>MW: Extract IP (trust_proxy check)
    alt Bearer token present
        MW->>DB: SELECT api_tokens WHERE token_hash = hash
        MW->>MW: Check expiry, extract scopes + boundaries
    else Session cookie present
        MW->>DB: SELECT auth_sessions WHERE token_hash = hash
        MW->>MW: Check expiry
    end
    MW->>MW: Build AuthUser struct

    API->>RBAC: require_project_read(state, auth, project_id)
    RBAC->>VK: GET perms:{user_id}:{project_id}
    alt Cache hit
        RBAC->>RBAC: Check permission in cached set
    else Cache miss
        RBAC->>DB: SELECT role_perms + delegations + workspace perms
        RBAC->>VK: SET perms:{user_id}:{project_id} (TTL 300s)
    end
    RBAC-->>API: Ok(()) or Err(NotFound)
```
<!-- /mermaid -->

---

## R6: Agent Session Lifecycle

Create session → spawn K8s pod → ephemeral identity → NDJSON streaming → reaper:

<!-- mermaid:diagrams/runtime-agent.mmd -->
```mermaid
sequenceDiagram
    participant User
    participant API as Session API
    participant Svc as agent::service
    participant ID as agent::identity
    participant DB as PostgreSQL
    participant VK as Valkey
    participant K8s as Kubernetes
    participant Pod as Agent Pod
    participant Claude as Claude API

    User->>API: POST /api/sessions {project_id, prompt, role}
    API->>Svc: create_session()

    Svc->>DB: INSERT agent_sessions (status=pending)
    Svc->>ID: create_ephemeral_identity(role)
    ID->>DB: INSERT users (type=agent)
    ID->>DB: INSERT user_roles (role per agent role)
    ID->>DB: INSERT delegations (time-bounded)
    ID->>DB: INSERT api_tokens (scoped to project)
    ID-->>Svc: agent_user_id + api_token

    Svc->>K8s: Create session namespace
    Svc->>K8s: Create registry pull/push secrets
    Svc->>VK: Configure Valkey ACL for session

    Note over Svc: Start pub/sub subscriber BEFORE pod
    Svc->>VK: Subscribe to agent:{session_id}

    Svc->>K8s: Create Pod (Claude Code image)
    Note over K8s: Pod env: PLATFORM_API_TOKEN,<br/>PLATFORM_API_URL, session config

    Svc->>DB: UPDATE status=running, pod_name, namespace

    Pod->>Claude: LLM inference (via Claude CLI)
    Pod->>VK: Publish NDJSON to agent:{session_id}
    VK-->>Svc: Forward messages to client

    Note over Svc: Reaper (30min idle timeout)
    Svc->>DB: UPDATE status=completed
    Svc->>K8s: Delete pod
    Svc->>ID: Cleanup ephemeral identity
```
<!-- /mermaid -->

---

## R7: Observability Pipeline

OTLP ingest → channel buffer → Postgres (hot) → Parquet/MinIO (cold) → query → alerts:

<!-- mermaid:diagrams/runtime-observe.mmd -->
```mermaid
flowchart TD
    subgraph Ingest["OTLP Ingest (HTTP/Protobuf)"]
        traces_in[POST /v1/traces]
        logs_in[POST /v1/logs]
        metrics_in[POST /v1/metrics]
    end

    subgraph Channels["Buffered Channels"]
        traces_ch[spans_tx channel]
        logs_ch[logs_tx channel]
        metrics_ch[metrics_tx channel]
    end

    subgraph Flush["Background Flush Tasks"]
        traces_flush["Traces flush<br/>1s / 500 buffer"]
        logs_flush["Logs flush<br/>1s / 500 buffer"]
        metrics_flush["Metrics flush<br/>1s / 500 buffer"]
    end

    subgraph Hot["Hot Storage (Postgres)"]
        traces_db[(traces + spans)]
        logs_db[(log_entries)]
        metrics_db[(metric_series + samples)]
    end

    subgraph Cold["Cold Storage (MinIO)"]
        parquet["Parquet files<br/>time-based rotation"]
    end

    subgraph Query["Query API"]
        q_traces[GET /api/observe/traces]
        q_logs[GET /api/observe/logs]
        q_metrics[GET /api/observe/metrics]
    end

    subgraph Alerts["Alert Evaluation"]
        eval[Periodic rule evaluation]
        notify_dispatch[Notification dispatch]
    end

    traces_in --> traces_ch --> traces_flush --> traces_db
    logs_in --> logs_ch --> logs_flush --> logs_db
    metrics_in --> metrics_ch --> metrics_flush --> metrics_db

    logs_flush -.->|"pub/sub live tail"| VK[Valkey]

    traces_db --> parquet
    logs_db --> parquet
    metrics_db --> parquet

    traces_db --> q_traces
    logs_db --> q_logs
    metrics_db --> q_metrics
    parquet --> q_traces
    parquet --> q_logs

    metrics_db --> eval --> notify_dispatch

    subgraph Self["Platform Self-Observability"]
        bridge[tracing_layer bridge]
    end
    bridge -.->|"warn+ logs"| logs_ch
```
<!-- /mermaid -->

---

## R8: State Machines

### Pipeline Status

<!-- mermaid:diagrams/state-pipeline.mmd -->
```mermaid
stateDiagram-v2
    [*] --> Pending

    state "Pipeline Status" as ps {
        Pending --> Running
        Pending --> Cancelled
        Running --> Success
        Running --> Failure
        Running --> Cancelled
        Success --> [*]
        Failure --> [*]
        Cancelled --> [*]
    }
```
<!-- /mermaid -->

### Pipeline Step Status

<!-- mermaid:diagrams/state-step.mmd -->
```mermaid
stateDiagram-v2
    [*] --> Pending
    Pending --> Running
    Pending --> Skipped
    Running --> Success
    Running --> Failure
    Success --> [*]
    Failure --> [*]
    Skipped --> [*]
```
<!-- /mermaid -->

### Release Phase (Deployment)

The most complex state machine — supports canary, rolling, and AB test strategies:

<!-- mermaid:diagrams/state-deployment.mmd -->
```mermaid
stateDiagram-v2
    [*] --> Pending

    Pending --> Progressing : Apply manifests
    Pending --> Completed : Rolling fast-path
    Pending --> Cancelled
    Pending --> Failed : Unrecoverable error

    Progressing --> Holding : Analysis gate
    Progressing --> Paused : Manual pause
    Progressing --> Promoting : All steps pass
    Progressing --> RollingBack : Analysis fail
    Progressing --> Failed

    Holding --> Progressing : Resume
    Holding --> RollingBack : Timeout/fail
    Holding --> Failed

    Paused --> Progressing : Resume
    Paused --> Cancelled : Manual cancel
    Paused --> RollingBack
    Paused --> Failed

    Promoting --> Completed : 100% traffic routed
    Promoting --> Failed

    RollingBack --> RolledBack : Rollback complete
    RollingBack --> Failed

    Completed --> [*]
    RolledBack --> [*]
    Cancelled --> [*]
    Failed --> [*]
```
<!-- /mermaid -->

### Agent Session Status

<!-- mermaid:diagrams/state-agent-session.mmd -->
```mermaid
stateDiagram-v2
    [*] --> Pending
    Pending --> Running : Pod created
    Running --> Completed : Normal exit
    Running --> Failed : Pod error
    Running --> Stopped : User/reaper stop
    Completed --> [*]
    Failed --> [*]
    Stopped --> [*]
```
<!-- /mermaid -->

---

## Additional Reference Diagrams

### Two-Repo Topology

How the code repo and ops repo relate during GitOps sync:

<!-- mermaid:diagrams/two-repo-topology.mmd -->
```mermaid
flowchart LR
    subgraph Code["Code Repo"]
        platform_yaml[.platform.yaml]
        deploy_dir["deploy/"]
        vars_staging["deploy/variables_staging.yaml"]
        vars_prod["deploy/variables_prod.yaml"]
        testinfra["testinfra/"]
        dockerfile[Dockerfile]
    end

    subgraph Ops["Ops Repo"]
        ops_platform["platform.yaml"]
        ops_deploy["deploy/ (copied)"]
        ops_staging["values/staging.yaml (merged)"]
        ops_prod["values/production.yaml (merged)"]
        staging_branch["staging branch"]
        main_branch["main branch (production)"]
    end

    platform_yaml -->|"gitops_sync copies"| ops_platform
    deploy_dir -->|"gitops_sync copies"| ops_deploy
    vars_staging -->|"merge with image_ref"| ops_staging
    vars_prod -->|"merge with image_ref"| ops_prod
    ops_staging --> staging_branch
    ops_prod --> main_branch
    staging_branch -->|"manual promote"| main_branch
```
<!-- /mermaid -->

### Pipeline Step Types

Overview of all step types and their execution model:

<!-- mermaid:diagrams/step-types.mmd -->
```mermaid
flowchart TD
    subgraph StepTypes["Pipeline Step Types"]
        cmd["command<br/>Run arbitrary container<br/>with setup commands"]
        ib["imagebuild<br/>Generate kaniko command<br/>inject secrets as --build-arg<br/>push to registry"]
        dt["deploy_test<br/>Create test namespace<br/>apply testinfra<br/>spawn test pod<br/>cleanup"]
        gs["gitops_sync<br/>Copy files to ops repo<br/>merge variables<br/>commit + publish event"]
        dw["deploy_watch<br/>Poll deploy_releases<br/>until terminal phase"]
    end

    cmd -->|"Spawns Pod"| pod1[User Image Pod]
    ib -->|"Spawns Pod"| pod2[Kaniko Pod]
    dt -->|"Spawns Pod"| pod3[Test Runner Pod]
    gs -->|"In-Process"| ip1[No Pod]
    dw -->|"In-Process"| ip2[No Pod]
```
<!-- /mermaid -->
