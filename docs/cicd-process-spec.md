# CI/CD Pipeline — Full Process Specification

This document maps the **complete lifecycle** of a code change from MR creation through production deployment. It covers what exists today, what's missing, and the target design.

Legend: **[EXISTS]** = implemented, **[GAP]** = not yet implemented, **[PARTIAL]** = partially implemented

---

## Overview: Two Pipelines, Two Repos

```
CODE REPO (project git)          OPS REPO (deploy git)
  feature/shop-app                  staging branch
  └─ .platform.yaml                 └─ deploy/
  └─ app/                           └─ platform.yaml
  └─ deploy/                        └─ values/staging.yaml
  └─ Dockerfile*                    main branch
  └─ testinfra/                     └─ deploy/
                                    └─ platform.yaml
                                    └─ values/production.yaml
```
USERNOTES: We need to provide a way for the dev agent to specify different values (like vars, or ressource limits) for stating and deploy. maybe variables files directly in code repo deploy/ folder like deploy/variables_staging variables_prod?


**Build Pipeline** — triggered by code repo events (MR open, push to main)
**Deploy Pipeline** — triggered by ops repo updates (new committs (push to staging, merge to main))

---

## Phase 1: MR Creation → Build Pipeline Trigger

### Trigger Chain
```
create_demo_project()
  → git commit on feature/shop-app
  → create MR via DB insert
  → run_mr_create_side_effects() [background]
    → resolve HEAD SHA of source branch
    → pipeline::trigger::on_mr()
      → read .platform.yaml from repo at SHA
      → parse + validate pipeline definition
      → match trigger: mr.actions contains "opened" ✓
      → INSERT pipelines + pipeline_steps rows
      → notify_executor() → pipeline_notify.notify_one()
```

### What happens **[EXISTS]**
1. MR row created in DB with `source_branch`, `target_branch`, `status=open`
2. Pipeline triggered automatically — no manual API call needed

USERNOTES: how does the pipeline get triggered? do we create a MR Created event via our pubsub? or do we call it somewhere in orchestration code manually?

3. `.platform.yaml` parsed and validated (steps, DAG, trigger conditions)

### What happens **[GAP]**
4. **Schema validation of complete `.platform.yaml`** before pipeline starts — currently validation is structural (serde parse + basic checks), but does NOT validate:
   - That referenced Dockerfiles exist in the repo at that SHA
   - That `deploy_test.manifests` directory exists
   - That `deploy.specs[].canary.stable_service` / `canary_service` are valid K8s service names and present in deploy folder
   - That `flags[].key` follows naming conventions
   - That `deploy/` directory exists when `deploy` config is present

---

## Phase 2: Build Pipeline Execution

### Pipeline Steps (from demo `.platform.yaml`)

| Step | Image | Trigger | Depends On | Purpose |
|------|-------|---------|------------|---------|
| `build-app` | kaniko | push, mr | — | Build production image |
| `build-dev-image` | kaniko | push, mr | — | **[AUTO-ADDED]** if `Dockerfile.dev` exists |
| `build-canary` | kaniko | push, mr | — | Build canary image |
| `build-test` | kaniko | mr only | — | Build test runner image |
| `e2e` | (deploy_test) | mr only | build-app, build-test | Deploy testinfra + run tests |

USERNOTES: lets provide a cleaner way to handle kaniko builds in platform yaml ( a combination between the to simply dev image build and the direct kaniko... )_
- image name should be provided
- image path (gets included automatically (registry and project id)) (currently if agent decide to delete registry or project var it pipeline failes somewhere..)
- registry (+ needed insecure flags..) will be provided. i am looking for something like this:
    - type: imagebuild
      imageName: app
      secrets:
        - SECRET_NEEDED_DURING_DOCKERFILE_BUILD (gets injected from platform project secrets, gets set via knaiko build args)
      (platform handles push credentials on default, already works)
      optional: registry/tag full overwrite, push credentials


### Step Execution Flow (per step)

```
execute_step_dispatch(step)
  │
  ├─ [EXISTS] Create project namespace: {namespace_slug}-dev
  │     └─ idempotent via ensure_namespace()
  │     └─ NetworkPolicy applied (egress to platform API + DNS + internet)
  │
  ├─ [EXISTS] Init container: git clone --depth 1 --branch <REF> <REPO_URL> /workspace
  │     └─ GIT_ASKPASS token (1hr, project-scoped)
  │
  ├─ [EXISTS] Main container runs step commands
  │     Environment variables injected:
  │     ┌─────────────────────────────────────────────────────────────────┐
  │     │ PLATFORM_PROJECT_ID, PLATFORM_PROJECT_NAME                     │ [EXISTS]
  │     │ PIPELINE_ID, STEP_NAME                                         │ [EXISTS]
  │     │ COMMIT_REF, COMMIT_BRANCH, COMMIT_SHA, SHORT_SHA               │ [EXISTS]
  │     │ IMAGE_TAG, PROJECT, VERSION, REGISTRY                          │ [EXISTS]
  │     │ PIPELINE_TRIGGER (push/mr/tag/api)                             │ [EXISTS]
  │     │ OTEL_EXPORTER_OTLP_ENDPOINT (platform API URL)                 │ [EXISTS]
  │     │ OTEL_SERVICE_NAME (project/step)                               │ [EXISTS]
  │     │ OTEL_RESOURCE_ATTRIBUTES (project_id)                          │ [EXISTS]
  │     │ OTEL_EXPORTER_OTLP_HEADERS (Bearer token, observe:write)       │ [EXISTS]
  │     │ Project secrets (scope: pipeline/agent/all, decrypted)          │ [EXISTS]
  │     └─────────────────────────────────────────────────────────────────┘
  │
  ├─ [EXISTS] Kaniko registry auth: /kaniko/.docker secret mounted
  │     └─ Docker config JSON for platform registry push
  │
  ├─ [EXISTS] Poll pod status every 3s (timeout: 900s)
  │     └─ Detect: ImagePullBackOff, CrashLoopBackOff, etc.
  │
  ├─ [EXISTS] Capture logs → MinIO: logs/pipelines/{pipeline_id}/{step_name}.log
  │
  └─ [EXISTS] Update step status: success/failure/skipped + exit_code + duration_ms
```
USERNOTES: pipeline should create ns: {namespace_slug}-pipeline-{id}
pipeline should get secret for platform and inject to kaniko build


### Deploy-Test Step (E2E) — Special Flow

```
execute_deploy_test_step(step)
  │
  ├─ [EXISTS] Create test namespace: {namespace_slug}-test-{pipeline_id[:8]}
  │
  ├─ [EXISTS] Read deploy manifests from testinfra/ directory
  │
  ├─ [EXISTS] Render manifests with "test" environment
  │
  ├─ [EXISTS] Apply manifests to test namespace (postgres, app, services)
  │
  ├─ [EXISTS] Wait for Deployment readiness (readiness_timeout: 120s)
  │
  ├─ [EXISTS] Wait for Services to have ready endpoints
  │     └─ wait_for_services: [platform-demo-app, platform-demo-db]
  │
  ├─ [GAP] OTEL token for test namespace — NOT created/injected
  │     └─ Test pods don't get OTEL env vars for sending telemetry
  │     └─ Should: create scoped token, inject as env vars to deployed manifests
  │
  ├─ [GAP] K8s secret injection for test namespace
  │     └─ Project secrets (scope: deploy) NOT synced to test namespace
  │     └─ Should: inject_project_secrets() for test namespace like reconciler does
  │
  ├─ [EXISTS] Spawn test pod with APP_HOST/APP_PORT env vars
  │
  └─ [EXISTS] Cleanup test namespace on exit
```

USERNOTES: secrets probaly need new scope ( no backwards comp needed): scopes are: all, agent, pipeline, test, staging, prod (all fall back to all...),

### DAG Execution (when `depends_on` present)

```
[EXISTS] Topological sort → find ready steps (in-degree=0)
[EXISTS] Spawn up to pipeline_max_parallel concurrent steps
[EXISTS] On step success: decrement dependents' in-degree, add newly-ready to queue
[EXISTS] On step failure: mark transitive dependents as skipped
[EXISTS] Per-step condition: step.only.events + step.only.branches (glob match)
```

### Pipeline Completion — Finalize

```
finalize_pipeline(pipeline_id, all_succeeded)
  │
  ├─ [EXISTS] Set status = success/failure, finished_at = now()
  │
  ├─ If success:
  │   ├─ [EXISTS] detect_and_write_deployment() → gitops_handoff (if main branch)
  │   ├─ [EXISTS] detect_and_publish_dev_image() → DevImageBuilt event
  │   └─ [EXISTS] try_auto_merge() → check eligible MRs
  │
  ├─ [EXISTS] fire_build_webhook()
  │
  └─ [EXISTS] emit_pipeline_log() → observe module
```

USERNOTES: detect_and_publish_dev_image should be a normal step (check new imagebuilt instrcutions)

---

USERNOTES: this is where the big picture falls apart and becomes to complex, due to "magic over explicitness".
Add a deployandWatch step to pipeline yaml: depends on gates pass (type deploy). this is still in MR mode but deploy should get called now...


## Phase 3: Post-Pipeline — MR Auto-Merge

### Current Flow **[EXISTS but requires setup]**

```
try_auto_merge(project_id)  [called after pipeline success]
  │
  ├─ Query: open MRs with auto_merge=true for project
  │
  ├─ For each eligible MR:
  │   ├─ Build synthetic AuthUser from auto_merge_by
  │   ├─ Check merge gates:
  │   │   ├─ Required approvals met?
  │   │   ├─ CI pipeline status = "success" on source branch?
  │   │   ├─ Source branch up-to-date with target?
  │   │   └─ Allowed merge method?
  │   └─ If all gates pass: do_merge()
  │
  └─ do_merge():
      ├─ [EXISTS] git worktree merge (--no-ff by default)
      ├─ [EXISTS] Update MR: status=merged, merge_commit_sha
      ├─ [EXISTS] Stop preview environments for branch
      ├─ [EXISTS] Fire webhooks: mr.merged
      └─ [EXISTS] Audit log
```

### Gap: Demo project doesn't enable auto-merge **[GAP]**

The demo project's `create_merge_request()` in `demo_project.rs` does NOT set `auto_merge=true` on the MR. So after MR pipeline succeeds, nothing auto-merges.

**Fix needed**: Set `auto_merge=true, auto_merge_by=owner_id` when creating the demo MR.

### Gap: Post-merge push doesn't trigger pipeline **[GAP]**

When `do_merge()` completes, the merge commit is created directly in the bare repo (via git worktree), NOT through the git HTTP push endpoint. Therefore **the post-receive hook never fires**, and no push-triggered pipeline runs on `main`.

**Fix needed**: After `do_merge()`, explicitly call `pipeline::trigger::on_push()` for the merge commit on `main`. This is the pipeline that builds images for production.

---

## Phase 4: Main Branch Pipeline → GitOps Handoff

### After merge to main, a push-triggered pipeline should run **[GAP — see above]**

Assuming the push-trigger gap is fixed, this pipeline runs:

| Step | Runs? | Why |
|------|-------|-----|
| `build-app` | YES | push trigger, branches: ["main"] |
| `build-canary` | YES | push trigger, no `only` filter |
| `build-test` | SKIPPED | `only: {events: [mr]}` |
| `e2e` | SKIPPED | `only: {events: [mr]}` + depends on build-test |
| `build-dev-image` | YES | auto-added, Dockerfile.dev exists |

### GitOps Handoff (after main pipeline success) **[EXISTS]**

```
detect_and_write_deployment()
  │
  ├─ Query kaniko steps with status=success
  │
  ├─ Branch = main? → gitops_handoff()
  │
  └─ gitops_handoff():
      │
      ├─ [EXISTS] Look up ops repo for project
      │
      ├─ [EXISTS] Read .platform.yaml from project repo at commit_sha
      │
      ├─ [EXISTS] Parse deploy config (enable_staging, specs)
      │
      ├─ [EXISTS] Sync deploy/ from project repo → ops repo
      │     └─ ops_repo::sync_from_project_repo()
      │
      ├─ [EXISTS] Write platform.yaml to ops repo root
      │
      ├─ [EXISTS] Determine target:
      │     └─ enable_staging=true → branch=staging, env=staging
      │     └─ enable_staging=false → branch=main, env=production
      │
      ├─ [EXISTS] Build values JSON:
      │     {
      │       "image_ref": "registry/project/app:SHA",
      │       "canary_image_ref": "registry/project/canary:SHA",
      │       "project_name": "platform-demo",
      │       "environment": "staging"
      │     }
      │
      ├─ [EXISTS] Commit values to ops repo on target branch
      │     └─ ops_repo::commit_values()
      │
      └─ [EXISTS] Publish OpsRepoUpdated event
            {
              project_id, ops_repo_id,
              environment: "staging",
              commit_sha: "<ops_commit>",
              image_ref: "registry/project/app:SHA"
            }
```

### What's NOT in gitops_handoff but user expects **[GAPs]**

1. **copyToOpsRepo as a visible pipeline step** — Currently gitops_handoff is internal executor logic. User wants it as an explicit pipeline step in `.platform.yaml` so the test can observe it.

2. **watchDeploy as a pipeline step** — User wants the pipeline to include a step that watches the deploy pipeline (triggered by ops repo update) and reports success/failure back. Currently pipeline finishes before deploy even starts.

3. **Variable file for jinja deploy variables** — gitops_handoff writes `values/{environment}.yaml` but doesn't copy env-specific variable files from the project repo. It only writes the image_ref/project_name/environment.

4. **Platform.yaml copied to ops repo** — This IS done (step 2 above). ✓

---

## Phase 5: Deploy Pipeline (OpsRepoUpdated → Reconciler)

### Event Handler **[EXISTS]**

```
handle_ops_repo_updated(project_id, environment, commit_sha, image_ref)
  │
  ├─ [EXISTS] Read .platform.yaml from ops repo at commit_sha
  │
  ├─ [EXISTS] Extract deploy strategy + rollout_config from specs[0]
  │
  ├─ [EXISTS] Detect canary image: check for "canary" in step names
  │
  ├─ [EXISTS] Upsert deploy_targets for environment
  │
  ├─ [EXISTS] Create deploy_releases row:
  │     strategy: "canary"
  │     phase: "pending"
  │     rollout_config: { steps, interval, progress_gates, rollback_triggers }
  │
  ├─ [EXISTS] Register feature flags from platform.yaml:
  │     INSERT INTO feature_flags ON CONFLICT DO NOTHING
  │
  ├─ [GAP] Feature flag pruning — keep flags from current + previous commit,
  │     delete all older flags for the project. Currently: only inserts new flags,
  │     never deletes old ones.
  │
  └─ [EXISTS] Wake reconciler: deploy_notify.notify_one()
```

### Reconciler: handle_pending() **[EXISTS]**

```
handle_pending(release)
  │
  ├─ [EXISTS] Target namespace: {namespace_slug}-{env_suffix}
  │     e.g., platform-demo-staging, platform-demo-prod
  │
  ├─ [EXISTS] ensure_namespace():
  │     └─ Create namespace with labels + NetworkPolicy
  │
  ├─ [EXISTS] inject_project_secrets():
  │     └─ Query secrets: scope IN ('deploy', 'all')
  │     └─ Filter by environment (NULL or matching)
  │     └─ Decrypt with PLATFORM_MASTER_KEY
  │     └─ Create K8s Secret: {namespace}-{env}-secrets
  │     └─ Contents:
  │         ┌─────────────────────────────────────────────────────────────┐
  │         │ User secrets (scope: deploy/all):                           │
  │         │   DATABASE_URL, VALKEY_URL, APP_SECRET_KEY, SENTRY_DSN      │
  │         │                                                             │
  │         │ OTEL env vars (auto-injected):                              │
  │         │   OTEL_EXPORTER_OTLP_ENDPOINT = platform API URL            │
  │         │   OTEL_SERVICE_NAME = project name                          │
  │         │   OTEL_RESOURCE_ATTRIBUTES = platform.project_id=UUID       │
  │         │   OTEL_EXPORTER_OTLP_HEADERS = Authorization=Bearer <token> │
  │         │                                                             │
  │         │ Platform tokens (auto-created):                             │
  │         │   PLATFORM_API_TOKEN = <project:read scoped token>          │
  │         │   PLATFORM_API_URL = platform API URL                       │
  │         │   PLATFORM_PROJECT_ID = project UUID                        │
  │         └─────────────────────────────────────────────────────────────┘
  │
  ├─ [EXISTS] ensure_scoped_tokens():
  │     └─ OTEL token: name=otlp-{scope}-{proj8}, scopes=[observe:write]
  │     └─ API token: name=api-{scope}-{proj8}, scopes=[project:read]
  │     └─ Reuse existing if not expired, else rotate
  │
  ├─ [EXISTS] ensure_registry_pull_secret():
  │     └─ platform-registry-pull secret in namespace
  │     └─ Docker auth for pulling from platform registry
  │
  ├─ [EXISTS] render_manifests():
  │     └─ Read manifests from ops repo at HEAD SHA
  │     └─ Load values/{environment}.yaml
  │     └─ Merge with values_override
  │     └─ Render via minijinja:
  │         image_ref, stable_image, commit_sha, canary_image_ref,
  │         platform_api_url, project_name, environment
  │
  ├─ [EXISTS] Inject envFromSecret reference into rendered Deployment YAML
  │
  ├─ [EXISTS] Track resources (parse rendered YAML → inventory)
  │
  ├─ [EXISTS] Prune orphaned resources from previous release
  │
  ├─ [EXISTS] applier::apply_manifests():
  │     └─ Server-side apply with platform-deployer field manager
  │     └─ Force namespace to deployment namespace (security)
  │     └─ Kind whitelist enforced
  │
  ├─ For rolling strategy:
  │   └─ [EXISTS] Wait for Deployment health (300s timeout)
  │   └─ [EXISTS] Transition to completed, health=healthy
  │
  ├─ For canary/ab_test strategy:
  │   └─ [EXISTS] Transition to progressing with initial traffic weight
  │   └─ [EXISTS] apply_gateway_resources():
  │         └─ Create Envoy Gateway
  │         └─ Create HTTPRoute with weighted split:
  │             stable: 100-weight%, canary: weight%
  │
  └─ [GAP] Readiness check / container health verification
      └─ Rolling strategy waits for Deployment, but does not explicitly check
         that containers are actually serving traffic (no HTTP probe)
      └─ Should: verify health endpoint returns 200 before marking complete
```

---

## Phase 6: Canary Progression (if strategy=canary)

### Analysis Loop **[EXISTS]** — `src/deployer/analysis.rs`

```
Every 15 seconds:
  │
  ├─ Query releases with phase IN (progressing, holding) AND strategy IN (canary, ab_test)
  │
  ├─ For each release:
  │   ├─ Ensure rollout_analyses record exists
  │   │
  │   ├─ Check rollback triggers (instant failure):
  │   │   └─ Evaluate metrics from observe module
  │   │   └─ If breached: verdict=fail → rolling_back
  │   │
  │   ├─ Evaluate progress gates:
  │   │   └─ Check metrics against thresholds
  │   │   └─ Pass: all gates satisfied
  │   │   └─ Fail: max_failures reached
  │   │   └─ Inconclusive: insufficient data
  │   │
  │   └─ Store verdict in rollout_analyses
  │
  └─ Reconciler reads verdicts:
      ├─ pass → advance step: next traffic weight from steps[]
      ├─ fail (count ≥ max_failures) → rolling_back
      └─ inconclusive → wait
```

### Canary Step Progression **[EXISTS]**

```
steps: [10, 25, 50, 100]

Step 0: 10% canary → evaluate progress_gates
Step 1: 25% canary → evaluate
Step 2: 50% canary → evaluate
Step 3: 100% canary → all gates pass → promoting
```

### Promotion **[EXISTS]**

```
handle_promoting():
  ├─ Route 100% to stable (canary becomes stable)
  ├─ Re-render manifests with stable_image = canary_image
  ├─ Apply updated manifests
  ├─ Transition to completed, health=healthy
  └─ Publish ReleasePromoted event
```

### Rollback **[EXISTS]**

```
handle_rolling_back():
  ├─ Route 0% to canary (100% to stable)
  ├─ Transition to rolled_back, health=unhealthy
  └─ Publish ReleaseRolledBack event
```

---

## Phase 7: Staging → Production Promotion

### Current Flow **[EXISTS]** — `src/api/deployments.rs`

```
POST /api/projects/{id}/promote-staging
  │
  ├─ Fetch ops repo for project
  ├─ Read staging values to extract image_ref
  ├─ Merge staging branch into production/main branch
  ├─ Publish OpsRepoUpdated event (environment=production)
  │     → triggers Phase 5 again for production
  ├─ Audit log + webhook
  └─ Return 200 with status=promoted
```

### What user expects **[GAP]**

The user wants staging → production promotion to be **automatic** after canary completes (no manual API call). Currently requires `POST /promote-staging`.

**Fix needed**: After canary release completes (phase=completed) in staging, automatically trigger production promotion. Could be:
- Reconciler detects completed staging release → calls promote_staging internally
- Or: eventbus handler on ReleasePromoted for staging → promotes to production

---

## Phase 8: Feature Flag Evaluation

### Flow **[EXISTS]**

```
POST /api/flags/evaluate
  Body: { project_id, keys: ["new_checkout_flow", "dark_mode"] }
  Auth: Bearer <PLATFORM_API_TOKEN> (injected via K8s secret)

  → Returns: { values: { "new_checkout_flow": false, "dark_mode": false } }
```

### Flag Registration **[EXISTS]**

- Flags in `.platform.yaml` → registered during OpsRepoUpdated handler
- `INSERT INTO feature_flags ON CONFLICT DO NOTHING`

### Flag Pruning **[GAP]**

User wants: keep flags from **current commit + previous commit**, delete all older flags for the project.
Currently: flags are only ever inserted, never deleted. Stale flags accumulate.

---

## Complete Gap Summary

### Critical Path Gaps (block the flow)

| # | Gap | Where | Impact |
|---|-----|-------|--------|
| G1 | **Demo MR not set to auto_merge=true** | `demo_project.rs` | MR never auto-merges after pipeline success |
| G2 | **Post-merge doesn't trigger main pipeline** | `merge_requests.rs do_merge()` | No push-triggered pipeline after merge (git hook bypassed) |
| G3 | **No `copyToOpsRepo` pipeline step** | `executor.rs` / `definition.rs` | GitOps handoff is invisible, happens internally. User wants it as a declared pipeline step. |
| G4 | **No `watchDeploy` pipeline step** | `executor.rs` / `definition.rs` | Pipeline completes before deploy starts. User wants pipeline to wait for deploy success. |

### Enhancement Gaps (system works but misses features)

| # | Gap | Where | Impact |
|---|-----|-------|--------|
| G5 | **Platform.yaml schema validation incomplete** | `definition.rs` | No check that referenced files exist at that SHA |
| G6 | **Test namespace missing OTEL tokens** | `executor.rs deploy_test` | Test pods can't send telemetry |
| G7 | **Test namespace missing project secrets** | `executor.rs deploy_test` | Test pods don't get DB URLs etc. |
| G8 | **Feature flag pruning** | `eventbus.rs` | Old flags never cleaned up |
| G9 | **Auto-promote staging→production** | `reconciler.rs` / `deployments.rs` | Requires manual API call |
| G10 | **Deploy readiness verification** | `reconciler.rs` | No HTTP health probe, only K8s readiness |
| G11 | **HTTPGateway for all deploy envs** | `gateway.rs` / `reconciler.rs` | Only created for canary/AB, not for rolling strategy |
| G12 | **Variable file for jinja deploy vars** | `executor.rs gitops_handoff` | Only writes image_ref/project_name, not env-specific config from project repo |

---

## Target: Full Auto-Triggered Flow (No Manual API Calls)

```
1. create_demo_project()
   └─ Creates project, repos, secrets, issues
   └─ Creates feature branch + commits demo app
   └─ Creates MR with auto_merge=true          ← [G1 fix]

2. MR pipeline auto-triggers (on_mr)
   └─ build-app, build-canary, build-test, e2e
   └─ All steps complete → pipeline success

3. Auto-merge fires (try_auto_merge)
   └─ Pipeline succeeded ✓, auto_merge=true ✓
   └─ do_merge() → merge to main

4. Post-merge triggers main pipeline            ← [G2 fix]
   └─ pipeline::trigger::on_push(main, merge_sha)
   └─ build-app, build-canary run
   └─ build-test, e2e skipped (push-only)
   └─ build-dev-image auto-added

5. Main pipeline success → gitops_handoff
   └─ Copy deploy/ + platform.yaml to ops repo
   └─ Commit values to staging branch
   └─ Publish OpsRepoUpdated(staging)

6. Eventbus → handle_ops_repo_updated
   └─ Create deploy_releases(staging, canary)
   └─ Register feature flags
   └─ Wake reconciler

7. Reconciler processes staging release
   └─ Create namespace: platform-demo-staging
   └─ Inject secrets + OTEL tokens
   └─ Registry pull secret
   └─ Render manifests from ops repo
   └─ Apply to K8s
   └─ Canary progression via analysis loop
   └─ Complete → auto-promote to production     ← [G9 fix]

8. Production deployment
   └─ OpsRepoUpdated(production)
   └─ Same reconciler flow for production
   └─ Canary progression → completed

9. Feature flags evaluable via API
   └─ Apps use PLATFORM_API_TOKEN to call /api/flags/evaluate
```

---

## Proposed: `copyToOpsRepo` + `watchDeploy` Pipeline Steps

### Option A: Built-in step types (recommended)

Add two new step types to `.platform.yaml`:

```yaml
pipeline:
  steps:
    - name: build-app
      image: kaniko
      commands: [...]

    - name: build-canary
      image: kaniko
      commands: [...]

    # New: explicit ops repo sync step
    - name: sync-ops-repo
      type: gitops_sync           # ← new step type
      depends_on: [build-app, build-canary]
      only:
        events: [push]
        branches: ["main"]
      gitops:
        copy: [deploy/, platform.yaml]     # files to copy from code repo
        values:                            # values to write
          image_ref: $REGISTRY/$PROJECT/app:$COMMIT_SHA
          canary_image_ref: $REGISTRY/$PROJECT/canary:$COMMIT_SHA

    # New: wait for deploy to complete
    - name: watch-deploy
      type: deploy_watch           # ← new step type
      depends_on: [sync-ops-repo]
      only:
        events: [push]
        branches: ["main"]
      deploy_watch:
        environment: staging       # which environment to watch
        timeout: 300               # seconds to wait for completion
```

### Option B: Keep internal (current approach + observability)

Keep gitops_handoff as internal executor logic but:
- Create synthetic pipeline_steps rows for visibility
- Emit structured logs that the test can poll
- Publish events the test can subscribe to

---

## Appendix: Key Files

| File | Purpose |
|------|---------|
| `src/pipeline/definition.rs` | `.platform.yaml` parsing + validation |
| `src/pipeline/trigger.rs` | Pipeline trigger matching (on_push, on_mr, on_tag) |
| `src/pipeline/executor.rs` | Step execution, gitops_handoff, finalize |
| `src/deployer/reconciler.rs` | Release state machine, namespace/secret setup |
| `src/deployer/ops_repo.rs` | Ops repo management (sync, read, write) |
| `src/deployer/applier.rs` | K8s manifest application |
| `src/deployer/renderer.rs` | Manifest rendering (minijinja) |
| `src/deployer/gateway.rs` | HTTPRoute/Gateway for canary/AB |
| `src/deployer/analysis.rs` | Canary metric analysis loop |
| `src/store/eventbus.rs` | Event publish/subscribe + handlers |
| `src/api/merge_requests.rs` | MR lifecycle, auto-merge, post-merge side effects |
| `src/api/deployments.rs` | Deploy targets, releases, promote-staging |
| `src/api/flags.rs` | Feature flag CRUD + evaluation |
| `src/onboarding/demo_project.rs` | Demo project bootstrap |
| `src/secrets/mod.rs` | Secret encryption/decryption |
