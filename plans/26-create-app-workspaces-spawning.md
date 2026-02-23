# Plan 26 — Create App UX, Per-User Keys, Workspaces, Secrets Hierarchy, Agent Spawning

## Context

The platform has a working end-to-end flow (account → project → agent → pipeline → deploy), but the dashboard UX is passive — just stats and project lists. The primary ask is a **"Create App" experience** where a user describes an idea, an AI agent clarifies requirements conversationally, then spawns sub-agents to scaffold the repo, ops repo, and CI/CD config. This requires several foundational changes:

- **Per-user API keys**: Users bring their own Anthropic key (currently one global K8s secret)
- **Workspaces**: Group users + projects (simpler precursor to full org/team hierarchy)
- **Secrets hierarchy**: Environment-aware secrets with workspace → project → project+env inheritance
- **Agent-to-agent spawning**: Agents can create child agents with role whitelist + permission cap

User decisions:
- Create App starts **project-less** (agent creates project during conversation)
- Agent spawn policy: **role whitelist** per parent session
- Org model: **simple Workspace** first (not full org+team)
- User provider keys: **separate table** from shared secrets

---

## Phase A: Per-User Provider Keys

**Goal**: Users store their own Anthropic API key. Agent pods use user key first, fall back to global K8s secret.

### A1. Migration

`migrations/20260223010001_user_provider_keys.up.sql`

```sql
CREATE TABLE user_provider_keys (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider      TEXT NOT NULL DEFAULT 'anthropic',
    encrypted_key BYTEA NOT NULL,
    key_suffix    TEXT NOT NULL,        -- last 4 chars for display
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, provider)
);
```

### A2. New file: `src/secrets/user_keys.rs`

CRUD using existing `engine::encrypt()` / `engine::decrypt()`:
- `set_user_key(pool, master_key, user_id, provider, api_key) -> Result<()>`
- `get_user_key(pool, master_key, user_id, provider) -> Result<Option<String>>`
- `delete_user_key(pool, user_id, provider) -> Result<bool>`
- `list_user_keys(pool, user_id) -> Result<Vec<KeyMetadata>>` (suffix only, never decrypted value)

### A3. API Endpoints

Add to `src/api/users.rs` or new `src/api/user_keys.rs`:

| Method | Path | Auth | Notes |
|--------|------|------|-------|
| PUT | `/api/users/me/provider-keys/{provider}` | Self | Body: `{ "api_key": "sk-ant-..." }` |
| GET | `/api/users/me/provider-keys` | Self | Returns `[{ provider, key_suffix, updated_at }]` |
| DELETE | `/api/users/me/provider-keys/{provider}` | Self | |

Validation: `check_length("api_key", &key, 10, 500)`, `check_name(provider)`.

### A4. Modify Agent Pod Creation

Files to modify:
- `src/agent/provider.rs` — add `anthropic_api_key: Option<String>` to `BuildPodParams`
- `src/agent/service.rs` — in `create_session()`, before building pod: check `user_keys::get_user_key(pool, master_key, user_id, "anthropic")`. Pass to `BuildPodParams`.
- `src/agent/claude_code/pod.rs` — in env var construction: if `params.anthropic_api_key` is `Some`, use plain `EnvVar { value }`. Otherwise keep existing `SecretKeySelector` from `platform-agent-secrets`.

### A5. UI

Settings section (or `/settings` page):
- Paste API key field (masked input)
- Show suffix if set
- Delete button

### A6. Tests

- Unit: encrypt/decrypt roundtrip for user keys
- Unit: pod builder produces correct env var for both code paths
- Integration (`#[sqlx::test]`): set/get/delete CRUD

---

## Phase B: Workspaces

**Goal**: Group users + projects under a workspace. Simpler precursor to full org/team.

### B1. Migration

`migrations/20260223020001_workspaces.up.sql`

```sql
CREATE TABLE workspaces (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name         TEXT NOT NULL UNIQUE,
    display_name TEXT,
    description  TEXT,
    owner_id     UUID NOT NULL REFERENCES users(id),
    is_active    BOOLEAN NOT NULL DEFAULT true,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workspace_members (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role         TEXT NOT NULL DEFAULT 'member'
                 CHECK (role IN ('owner', 'admin', 'member')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, user_id)
);
```

`migrations/20260223020002_project_workspace.up.sql`

```sql
ALTER TABLE projects
    ADD COLUMN workspace_id UUID REFERENCES workspaces(id);

CREATE INDEX idx_projects_workspace ON projects(workspace_id)
    WHERE workspace_id IS NOT NULL;
```

Design: Projects can optionally belong to a workspace. Existing `owner_id` projects continue working (backward-compatible). When a project has a `workspace_id`, workspace members get implicit access.

### B2. New Permissions

Add to `src/rbac/types.rs`:
```rust
WorkspaceRead,   // "workspace:read"
WorkspaceWrite,  // "workspace:write"
WorkspaceAdmin,  // "workspace:admin"
```

Seed in `src/store/bootstrap.rs`. Add to `admin` system role.

Bootstrap migration: `migrations/20260223020003_workspace_permissions.up.sql`

### B3. New module: `src/workspace/`

- `mod.rs` — re-exports
- `types.rs` — `Workspace`, `WorkspaceMember`, `WorkspaceRole` structs
- `service.rs` — CRUD for workspaces and memberships

### B4. API Endpoints

New file: `src/api/workspaces.rs`

| Method | Path | Permission |
|--------|------|------------|
| POST | `/api/workspaces` | Any authenticated |
| GET | `/api/workspaces` | `workspace:read` or member |
| GET | `/api/workspaces/{id}` | `workspace:read` or member |
| PATCH | `/api/workspaces/{id}` | Workspace owner/admin |
| DELETE | `/api/workspaces/{id}` | Workspace owner |
| POST | `/api/workspaces/{id}/members` | Workspace admin+ |
| GET | `/api/workspaces/{id}/members` | Workspace member |
| DELETE | `/api/workspaces/{id}/members/{user_id}` | Workspace admin+ |
| GET | `/api/workspaces/{id}/projects` | Workspace member |

Register in `src/api/mod.rs`.

### B5. Modify Project Creation

In `src/api/projects.rs`, `CreateProjectRequest` gains:
```rust
pub workspace_id: Option<Uuid>,
```
Validation: if provided, user must be workspace member. Project gets `workspace_id` set.

### B6. RBAC Integration

Extend `src/rbac/resolver.rs` — when resolving project permissions, if project has `workspace_id`, check workspace membership:
- Workspace `admin` → implicit `project:read` + `project:write`
- Workspace `member` → implicit `project:read`

### B7. UI

- Workspace selector in nav sidebar
- `/workspaces` list page
- Workspace detail with member management
- Project creation form gains optional workspace selector

### B8. Tests

- Integration: workspace CRUD + membership
- Integration: project creation with workspace
- Integration: permission resolution through workspace membership

---

## Phase C: Secrets Hierarchy

**Goal**: Secrets support workspace, project, and project+environment scoping. More specific wins.

### C1. Migration

`migrations/20260223030001_secrets_hierarchy.up.sql`

```sql
ALTER TABLE secrets
    ADD COLUMN workspace_id UUID REFERENCES workspaces(id),
    ADD COLUMN environment  TEXT CHECK (environment IS NULL OR
                            environment IN ('preview', 'staging', 'production'));

-- Replace old unique constraint with hierarchical one
ALTER TABLE secrets DROP CONSTRAINT IF EXISTS secrets_project_id_name_key;

-- New uniqueness: (workspace, project, environment, name)
CREATE UNIQUE INDEX idx_secrets_scoped ON secrets (
    COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000'::uuid),
    COALESCE(project_id,   '00000000-0000-0000-0000-000000000000'::uuid),
    COALESCE(environment,  '__none__'),
    name
);

-- Keep global index working
DROP INDEX IF EXISTS idx_secrets_global_name;
CREATE UNIQUE INDEX idx_secrets_global_name ON secrets(name)
    WHERE project_id IS NULL AND workspace_id IS NULL AND environment IS NULL;
```

### C2. Resolution Order (most specific wins)

1. Project + Environment (`project_id=X, environment=staging`)
2. Project (`project_id=X, environment IS NULL`)
3. Workspace (`workspace_id=W, project_id IS NULL`)
4. Global (`all NULLs`)

### C3. Modify `src/secrets/engine.rs`

Add `resolve_secret_hierarchical()`:

```rust
pub async fn resolve_secret_hierarchical(
    pool: &PgPool,
    master_key: &[u8; 32],
    project_id: Uuid,
    workspace_id: Option<Uuid>,
    environment: Option<&str>,
    name: &str,
    requested_scope: &str,
) -> anyhow::Result<String>
```

SQL: query all matching rows, ORDER BY specificity, LIMIT 1.

Keep existing `resolve_secret()` working (backward-compatible).

### C4. API Changes

Modify `src/api/secrets.rs`:
- `CreateSecretRequest` gains `environment: Option<String>`
- `GET /api/projects/{id}/secrets` gains `?environment=staging` filter
- New: `POST /api/workspaces/{id}/secrets` — workspace-scoped secrets
- New: `GET /api/workspaces/{id}/secrets` — list workspace secrets

### C5. Pipeline & Deploy Integration

Modify secret resolution in pipeline executor and deployer to call `resolve_secret_hierarchical()`, passing the deployment environment.

### C6. Tests

- Unit: hierarchy priority resolution
- Integration: create secrets at multiple levels, verify correct winner
- Integration: environment-specific override

---

## Phase D: Agent-to-Agent Spawning

**Goal**: Agents with `agent:spawn` can create child sessions. Role whitelist controls what children can be. Parent-child tracked.

### D1. Migration

`migrations/20260223040001_agent_spawning.up.sql`

```sql
ALTER TABLE agent_sessions
    ADD COLUMN parent_session_id UUID REFERENCES agent_sessions(id),
    ADD COLUMN spawn_depth       INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN allowed_child_roles TEXT[];  -- e.g. {'dev','ops'}

ALTER TABLE agent_sessions
    ADD CONSTRAINT chk_spawn_depth CHECK (spawn_depth <= 5);

CREATE INDEX idx_sessions_parent ON agent_sessions(parent_session_id)
    WHERE parent_session_id IS NOT NULL;
```

### D2. New Permission

Add `AgentSpawn` to `src/rbac/types.rs`:
```rust
AgentSpawn, // "agent:spawn"
```

Seed via migration `migrations/20260223040002_agent_spawn_permission.up.sql`.

### D3. Spawn Logic

In `src/agent/service.rs`, add `spawn_child_session()`:
1. Fetch parent session → get `project_id`, original `user_id`, `spawn_depth`
2. Check `spawn_depth < 5`
3. Check caller has `agent:spawn` on project
4. Check requested child role is in parent's `allowed_child_roles`
5. Create child session with `parent_session_id`, `spawn_depth + 1`
6. Child's delegator = parent's original human user (no escalation)
7. Child's permissions ≤ parent's permissions

### D4. Modify `UserType::can_spawn_agents()`

Change to `matches!(self, Self::Human | Self::Agent)` — actual gating is the `agent:spawn` permission.

### D5. API

| Method | Path | Permission |
|--------|------|------------|
| POST | `/api/projects/{id}/sessions/{sid}/spawn` | `agent:spawn` |
| GET | `/api/projects/{id}/sessions/{sid}/children` | `project:read` |

Spawn request:
```json
{
  "prompt": "Set up CI/CD pipeline",
  "config": { "role": "ops" },
  "allowed_child_roles": ["dev"]
}
```

### D6. MCP Tool

Add `spawn_agent` tool to `mcp/servers/platform-core.js` (available to all roles, gated by permission):
- Calls `POST /api/projects/{project_id}/sessions/{session_id}/spawn`
- Uses `SESSION_ID` env var for parent reference

### D7. Agent Token Scope

In `src/agent/identity.rs` — when delegating `AgentSpawn` to the agent user, also include `"agent:spawn"` in the API token scopes so the token can call the spawn endpoint.

### D8. Tests

- Unit: spawn depth limit
- Unit: role whitelist enforcement
- Unit: permission non-escalation
- Integration: human → parent → child chain
- Integration: spawn depth 5 rejection

---

## Phase E: Dashboard "Create App" Chat

**Goal**: Dashboard has a "Create App" card that opens a chat window. Starts a project-less agent session. Agent clarifies the idea, creates project + ops repo + pipeline via MCP tools + sub-agents.

### E1. Global Agent Sessions

Currently `agent_sessions.project_id` is `NOT NULL`. We need project-less sessions for the create-app flow.

Migration: `migrations/20260223050001_global_sessions.up.sql`

```sql
ALTER TABLE agent_sessions
    ALTER COLUMN project_id DROP NOT NULL;
```

### E2. New Agent Role: `create-app`

MCP composition in `docker/entrypoint.sh`:
```
create-app: core + pipeline + issues + deploy + admin (subset)
```

The `create-app` role's MCP tools include:
- `create_project` (from platform-core or new)
- `create_ops_repo` (from platform-admin)
- `configure_pipeline` (create `.platform.yaml`)
- `spawn_agent` (from platform-core, for delegating work)

### E3. API Endpoint

New in `src/api/sessions.rs`:

| Method | Path | Permission |
|--------|------|------------|
| POST | `/api/create-app` | `project:write` + `agent:run` |

Request:
```json
{ "description": "I want to build a ...", "provider": "claude-code" }
```

Handler:
1. Rate limit (5 per 10 min per user)
2. Create agent session with `project_id = NULL`, role `create-app`
3. Grant agent: `project:write` (global, so it can create projects), `agent:spawn`, `deploy:promote`
4. Return session with WebSocket URL

When the agent creates a project via MCP, it calls `PATCH /api/sessions/{id}` to associate the session with the new project_id (new endpoint or extend existing).

### E4. Session Update Endpoint

| Method | Path | Permission |
|--------|------|------------|
| PATCH | `/api/sessions/{id}` | Session owner (agent or human) |

Body: `{ "project_id": "uuid" }` — allows the create-app agent to link the session to the newly created project.

### E5. Dashboard UI

In `ui/src/pages/Dashboard.tsx`:

Add above stats grid:
```tsx
<div class="create-app-card" onClick={openCreateApp}>
  <h3>Create New App</h3>
  <p>Describe your idea — AI will set up everything</p>
</div>
```

### E6. Chat Interface

New `ui/src/pages/CreateApp.tsx`:
1. Full-page chat interface
2. Initial text input: "What would you like to build?"
3. On submit → `POST /api/create-app`
4. Open WebSocket to `ws://.../sessions/{id}/ws`
5. Stream progress events (thinking, tool calls, sub-agent spawns)
6. Allow follow-up messages via `POST .../sessions/{id}/message`
7. When agent creates project → show link, optionally redirect

New `ui/src/components/ChatWindow.tsx`:
- Message list (user + assistant bubbles)
- Tool call indicators (file edits, API calls)
- Sub-agent spawn indicators
- Input box with send button

### E7. Tests

- Integration: create-app session without project_id
- Integration: session update to link project_id
- E2E: full create-app flow (requires kind cluster)

---

## Migration Summary

| Version | Name | Phase |
|---------|------|-------|
| 20260223010001 | user_provider_keys | A |
| 20260223020001 | workspaces | B |
| 20260223020002 | project_workspace | B |
| 20260223020003 | workspace_permissions | B |
| 20260223030001 | secrets_hierarchy | C |
| 20260223040001 | agent_spawning | D |
| 20260223040002 | agent_spawn_permission | D |
| 20260223050001 | global_sessions | E |

## New/Modified Files Summary

### New Files
| File | Phase |
|------|-------|
| `src/secrets/user_keys.rs` | A |
| `src/api/user_keys.rs` | A |
| `src/workspace/mod.rs` | B |
| `src/workspace/types.rs` | B |
| `src/workspace/service.rs` | B |
| `src/api/workspaces.rs` | B |
| `ui/src/pages/CreateApp.tsx` | E |
| `ui/src/components/ChatWindow.tsx` | E |

### Modified Files
| File | Phases |
|------|--------|
| `src/rbac/types.rs` | B (+3 perms), D (+1 perm) |
| `src/store/bootstrap.rs` | B, D (seed perms/roles) |
| `src/api/mod.rs` | A, B, E (register routers) |
| `src/agent/provider.rs` | A (BuildPodParams) |
| `src/agent/service.rs` | A (user key), D (spawn), E (global session) |
| `src/agent/claude_code/pod.rs` | A (env var) |
| `src/agent/identity.rs` | D (permission cap) |
| `src/auth/user_type.rs` | D (Agent can spawn) |
| `src/secrets/engine.rs` | C (hierarchical resolution) |
| `src/secrets/mod.rs` | A (user_keys module) |
| `src/api/secrets.rs` | C (environment field, workspace endpoints) |
| `src/api/sessions.rs` | D (spawn), E (create-app, session update) |
| `src/api/projects.rs` | B (workspace_id field) |
| `src/rbac/resolver.rs` | B (workspace membership) |
| `docker/entrypoint.sh` | E (create-app role) |
| `mcp/servers/platform-core.js` | D (spawn_agent tool) |
| `ui/src/pages/Dashboard.tsx` | E (Create App card) |
| `Cargo.toml` | (no new deps expected) |

## Verification

Per phase:
- **A**: `just db-migrate && just db-prepare && just ci` — then manual test: set key via API, create session, verify pod spec
- **B**: Same CI flow + integration tests for workspace CRUD + permission resolution
- **C**: Integration tests: create secrets at workspace/project/env levels, verify resolution priority
- **D**: Integration tests: spawn chain, depth limit, role whitelist, permission cap
- **E**: Full flow: dashboard → chat → agent creates project → sub-agents scaffold → pipeline configured

All phases: `just ci` must pass (fmt + lint + deny + test-unit + build).
