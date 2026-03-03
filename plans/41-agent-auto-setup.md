# Plan 41: Agent Pod Auto-Setup

## Context

Currently, agent pods require a pre-built container image (`platform-claude-runner:latest`) with both the `agent-runner` CLI and Claude Code CLI pre-installed. This is inflexible — users who want custom base images or who don't pre-build the runner image can't launch agent sessions.

The platform already controls the pod startup command (via `src/agent/claude_code/pod.rs`), so it can inject setup logic at container start time. The goal is to make agent pods self-bootstrapping: the platform serves the `agent-runner` binary and installs Claude CLI automatically, so **any base image** with minimal deps (sh, node/npm) works as an agent image.

**Current state:**
- Pod spec: `command: ["agent-runner"]` — expects binary in image PATH
- `agent-runner` CLI: standalone Rust crate at `cli/agent-runner/` (~4.5 MB binary, 65 transitive deps, pure Rust/rustls)
- `Dockerfile.claude-runner`: installs Claude CLI via `npm install -g @anthropic-ai/claude-code` (unpinned), does NOT include agent-runner binary
- `entrypoint.sh`: calls `claude` directly (outdated, should call `agent-runner`)
- `agent-runner` finds claude binary via: `--cli-path` flag > `CLAUDE_CLI_PATH` env var > `which claude`

**Key insight:** agent-runner is small (~4.5 MB), pure Rust with no C deps beyond libc. Cross-compilation for linux/{amd64,arm64} is straightforward and fast (~30s).

## Design Principles

- **Any-image compatibility**: auto-setup works with any image that has `sh` and `node`/`npm` (for Claude CLI install) — no pre-built runner image required
- **Download from platform server**: agent-runner binary served by the platform HTTP API, not copied via kubectl or baked into every image
- **Version-pinned Claude CLI**: use fixed version via `npm install -g @anthropic-ai/claude-code@<version>`, configurable at platform level
- **Backward compatible**: pre-built images (`platform-claude-runner`) still work; setup detects existing tools and skips
- **Init container pattern**: setup runs in a dedicated init container (after git-clone) using the same image as the main container — clean failure if setup fails

---

## PR 1: Cross-compile agent-runner + download endpoint

Compile agent-runner for linux/amd64 and linux/arm64 during the platform Docker build, then serve via a new download API endpoint.

- [x] Types & errors defined
- [x] Dockerfile updated with cross-compilation stage
- [x] Download endpoint implemented
- [x] Tests written (red phase)
- [x] Implementation complete (green phase)
- [x] Integration tests passing
- [x] Quality gate passed

### Dockerfile: Cross-compilation stage

Add a new build stage to `docker/Dockerfile` between the UI builder and the main builder:

```dockerfile
# Stage 2.5: Cross-compile agent-runner for both architectures
FROM rust:1.88-slim-bookworm AS agent-runner-builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
    gcc-x86-64-linux-gnu libc6-dev-amd64-cross \
  && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
WORKDIR /agent-runner
COPY cli/agent-runner/ .
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc
RUN cargo build --release --target x86_64-unknown-linux-gnu
RUN cargo build --release --target aarch64-unknown-linux-gnu
```

In the runtime stage, add:
```dockerfile
COPY --from=agent-runner-builder \
  /agent-runner/target/x86_64-unknown-linux-gnu/release/agent-runner \
  /data/agent-runner/amd64
COPY --from=agent-runner-builder \
  /agent-runner/target/aarch64-unknown-linux-gnu/release/agent-runner \
  /data/agent-runner/arm64
```

**Size impact**: ~9 MB added to platform runtime image (2x ~4.5 MB binaries).

### Config additions

In `src/config.rs`, add two new fields:

```rust
/// Directory containing cross-compiled agent-runner binaries.
/// Expected layout: {dir}/amd64, {dir}/arm64
pub agent_runner_dir: PathBuf,

/// Claude CLI version for auto-setup in agent pods.
/// Used by the setup init container to pin `npm install -g @anthropic-ai/claude-code@<version>`.
pub claude_cli_version: String,
```

| Env var | Default | Purpose |
|---|---|---|
| `PLATFORM_AGENT_RUNNER_DIR` | `/data/agent-runner` | Directory with cross-compiled agent-runner binaries |
| `PLATFORM_CLAUDE_CLI_VERSION` | `stable` | Claude CLI version for npm install in agent pods |

### New endpoint: `GET /api/downloads/agent-runner`

**File**: `src/api/downloads.rs` (new file)

```rust
/// Query params for agent-runner download
#[derive(Debug, serde::Deserialize)]
pub struct DownloadParams {
    /// Target architecture: "amd64" or "arm64"
    pub arch: String,
}

/// GET /api/downloads/agent-runner?arch=amd64
///
/// Serves the cross-compiled agent-runner binary for the requested architecture.
/// Auth: Bearer token (agent pods have PLATFORM_API_TOKEN).
async fn download_agent_runner(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(params): Query<DownloadParams>,
) -> Result<Response, ApiError> {
    // Validate arch
    let arch = match params.arch.as_str() {
        "amd64" | "x86_64" => "amd64",
        "arm64" | "aarch64" => "arm64",
        _ => return Err(ApiError::Validation("arch must be 'amd64' or 'arm64'".into())),
    };

    let binary_path = state.config.agent_runner_dir.join(arch);
    let data = tokio::fs::read(&binary_path).await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("agent-runner binary not found for {arch}: {e}")))?;

    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/octet-stream"));
    headers.insert("content-disposition", HeaderValue::from_static("attachment; filename=\"agent-runner\""));
    headers.insert("cache-control", HeaderValue::from_static("public, max-age=3600"));

    Ok((StatusCode::OK, headers, data).into_response())
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/downloads/agent-runner", axum::routing::get(download_agent_runner))
}
```

Wire into `src/api/mod.rs`:
```rust
.merge(downloads::router())
```

**Security**: requires Bearer auth (AuthUser extractor). Agent pods get `PLATFORM_API_TOKEN` as env var. No sensitive data in the response.

**Note**: This endpoint reads from disk (not `include_bytes!`). The binary is ~4.5 MB — acceptable to load into memory for the response. For very large files, a streaming response via `ReaderStream` would be better, but this is fine for now.

### Code Changes — PR 1

| File | Change |
|---|---|
| `docker/Dockerfile` | Add `agent-runner-builder` stage; copy binaries to runtime image |
| `src/config.rs` | Add `agent_runner_dir: PathBuf`, `claude_cli_version: String` fields + env parsing |
| `src/api/downloads.rs` | New file: download endpoint |
| `src/api/mod.rs` | Add `pub mod downloads;` and `.merge(downloads::router())` |
| `src/main.rs` | No changes (router composition picks up new merge) |
| `tests/helpers/mod.rs` | Add `agent_runner_dir`, `claude_cli_version` to test Config |
| `tests/e2e_helpers/mod.rs` | Same |
| `.env.example` | Add `PLATFORM_AGENT_RUNNER_DIR`, `PLATFORM_CLAUDE_CLI_VERSION` |

### Test Outline — PR 1

**New behaviors to test:**
- `download_agent_runner` returns 200 with binary data for valid arch — integration
- `download_agent_runner` returns 400 for invalid arch — integration
- `download_agent_runner` returns 401 without auth — integration
- `download_agent_runner` normalizes arch aliases (x86_64→amd64, aarch64→arm64) — unit
- Config parsing with defaults — unit

**Error paths to test:**
- Binary not found on disk → 500 with descriptive error — integration
- Missing `arch` query param → 400 — integration

**Existing tests affected:**
- `tests/helpers/mod.rs` — add new Config fields
- `tests/e2e_helpers/mod.rs` — add new Config fields

**Estimated test count:** ~3 unit + 5 integration

---

## PR 2: Setup init container + pod spec update

Add a `setup-tools` init container that downloads agent-runner from the platform server and installs Claude CLI via npm. Update the main container command to use the workspace-installed tools.

- [x] Types & errors defined
- [x] Pod spec updated with setup init container
- [x] Tests written (red phase)
- [x] Implementation complete (green phase)
- [x] Integration/E2E tests passing
- [x] Quality gate passed

### Setup init container design

**Insert between `git-clone` and optional `setup` init containers.**

The init container:
1. Uses the **same image** as the main container (so it's already pulled, and matches the user's toolchain)
2. Has `PLATFORM_API_TOKEN`, `PLATFORM_API_URL`, `CLAUDE_CLI_VERSION` env vars
3. Runs a shell script that:
   - Creates `/workspace/.platform/bin/`
   - Detects architecture via `uname -m`
   - Downloads agent-runner from platform server (Node.js `fetch()` or `curl`)
   - Installs Claude CLI via `npm install -g --prefix /workspace/.platform @anthropic-ai/claude-code@<version>`
   - Skips each step if the tool already exists (backward compat)

**Shell script (generated in pod.rs):**

```sh
set -eu
BIN_DIR=/workspace/.platform/bin
mkdir -p "$BIN_DIR"

# 1. Download agent-runner from platform server
if [ ! -x "$BIN_DIR/agent-runner" ]; then
  ARCH=$(uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')
  echo "[setup] Downloading agent-runner ($ARCH)..."
  if command -v curl >/dev/null 2>&1; then
    curl -sf -H "Authorization: Bearer $PLATFORM_API_TOKEN" \
      "${PLATFORM_API_URL}/api/downloads/agent-runner?arch=${ARCH}" \
      -o "$BIN_DIR/agent-runner"
  elif command -v node >/dev/null 2>&1; then
    node -e "
      const fs = require('fs');
      const url = process.env.PLATFORM_API_URL + '/api/downloads/agent-runner?arch=${ARCH}';
      fetch(url, {headers:{'Authorization':'Bearer '+process.env.PLATFORM_API_TOKEN}})
        .then(r => { if(!r.ok) throw new Error('HTTP '+r.status); return r.arrayBuffer(); })
        .then(b => fs.writeFileSync('$BIN_DIR/agent-runner', Buffer.from(b)))
        .catch(e => { console.error(e); process.exit(1); });
    "
  else
    echo '[setup] ERROR: need curl or node to download agent-runner' >&2
    exit 1
  fi
  chmod +x "$BIN_DIR/agent-runner"
  echo "[setup] agent-runner installed"
fi

# 2. Install Claude CLI via npm (if npm available and claude not found)
if ! command -v claude >/dev/null 2>&1 && [ ! -x "$BIN_DIR/claude" ]; then
  if command -v npm >/dev/null 2>&1; then
    echo "[setup] Installing Claude CLI v${CLAUDE_CLI_VERSION:-stable}..."
    npm install -g --prefix /workspace/.platform \
      @anthropic-ai/claude-code@${CLAUDE_CLI_VERSION:-stable} 2>&1 | tail -1
    echo "[setup] Claude CLI installed"
  elif command -v curl >/dev/null 2>&1; then
    echo "[setup] Installing Claude CLI via native installer..."
    export HOME=/workspace/.platform
    curl -fsSL https://claude.ai/install.sh | bash -s "${CLAUDE_CLI_VERSION:-stable}"
    # Native installer puts binary at $HOME/.local/bin/claude
    if [ -x /workspace/.platform/.local/bin/claude ]; then
      ln -sf /workspace/.platform/.local/bin/claude "$BIN_DIR/claude"
    fi
    echo "[setup] Claude CLI installed"
  else
    echo '[setup] WARNING: no npm or curl — Claude CLI not installed' >&2
    echo '[setup] Ensure claude is available on PATH in the main container' >&2
  fi
fi

echo "[setup] Auto-setup complete"
```

**Key design choices:**
- **Tries `curl` first, falls back to `node`** for downloading agent-runner (handles images with or without curl)
- **Tries `npm` first, falls back to native installer** for Claude CLI (npm is most reliable; native installer needs bash)
- **Idempotent**: skips each tool if already present
- **Uses same image as main container**: no extra image pull, inherits the user's toolchain

### Pod spec changes in `pod.rs`

**1. New function: `build_setup_tools_container()`**

```rust
fn build_setup_tools_container(
    image: &str,
    pull_policy: &str,
    api_token: &str,
    api_url: &str,
    claude_cli_version: &str,
) -> Container {
    let setup_script = format!(
        r#"set -eu
BIN_DIR=/workspace/.platform/bin
mkdir -p "$BIN_DIR"
# ... (full script as above, with {claude_cli_version} interpolated)
"#
    );

    Container {
        name: "setup-tools".into(),
        image: Some(image.to_owned()),
        image_pull_policy: Some(pull_policy.to_owned()),
        command: Some(vec!["sh".into(), "-c".into()]),
        args: Some(vec![setup_script]),
        env: Some(vec![
            env_var("PLATFORM_API_TOKEN", api_token),
            env_var("PLATFORM_API_URL", api_url),
            env_var("CLAUDE_CLI_VERSION", claude_cli_version),
        ]),
        working_dir: Some("/workspace".into()),
        volume_mounts: Some(vec![workspace_mount()]),
        security_context: Some(container_security()),
        resources: Some(ResourceRequirements {
            requests: Some(BTreeMap::from([
                ("cpu".into(), Quantity("100m".into())),
                ("memory".into(), Quantity("256Mi".into())),
            ])),
            limits: Some(BTreeMap::from([
                ("cpu".into(), Quantity("500m".into())),
                ("memory".into(), Quantity("512Mi".into())),
            ])),
            ..Default::default()
        }),
        ..Default::default()
    }
}
```

**2. Update `build_init_containers()`**

Insert `setup-tools` after `git-clone`, before optional `setup`:

```rust
fn build_init_containers(params: &PodBuildParams<'_>) -> Vec<Container> {
    let resolved_image = resolve_image(params.config, params.project_agent_image, params.registry_url);
    let pull_policy = image_pull_policy(&resolved_image);

    let mut containers = vec![
        build_git_clone_container(params.repo_clone_url, &branch, params.agent_api_token),
        build_setup_tools_container(
            &resolved_image,
            &pull_policy,
            params.agent_api_token,
            params.platform_api_url,
            &params.claude_cli_version,
        ),
    ];

    // Optional project setup commands (runs after tools are installed)
    if let Some(ref commands) = params.config.setup_commands && !commands.is_empty() {
        // ... existing setup container ...
    }

    containers
}
```

**3. Update `build_main_container()`**

Change the command from `["agent-runner"]` to use the workspace-installed binary:

```rust
fn build_main_container(...) -> Container {
    Container {
        command: Some(vec!["/workspace/.platform/bin/agent-runner".to_owned()]),
        // ... rest unchanged ...
    }
}
```

**4. Update `build_agent_runner_args()`**

Add `--cli-path` to tell agent-runner where Claude CLI is:

```rust
fn build_agent_runner_args(params: &PodBuildParams<'_>) -> Vec<String> {
    let mut args = vec![
        "--cli-path".to_owned(),
        "/workspace/.platform/bin/claude".to_owned(),
        "--prompt".to_owned(),
        params.session.prompt.clone(),
        // ... rest unchanged ...
    ];
    // ...
}
```

**5. Update `build_env_vars()`**

Add headless operation env vars:

```rust
// Headless Claude CLI operation
vars.push(env_var("DISABLE_AUTOUPDATER", "1"));
vars.push(env_var("DISABLE_TELEMETRY", "1"));
// Ensure workspace tools are on PATH
vars.push(env_var("PATH", "/workspace/.platform/bin:/workspace/.platform/node_modules/.bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"));
```

Add `CLAUDE_CLI_VERSION` and `DISABLE_AUTOUPDATER` to the reserved env vars list.

**6. Update `PodBuildParams`**

Add `claude_cli_version` field:

```rust
pub struct PodBuildParams<'a> {
    // ... existing fields ...
    /// Claude CLI version for auto-setup (e.g., "stable", "2.1.63")
    pub claude_cli_version: &'a str,
}
```

**7. Update `src/agent/service.rs`**

Pass `claude_cli_version` from config when building pod params:

```rust
claude_cli_version: &state.config.claude_cli_version,
```

### Code Changes — PR 2

| File | Change |
|---|---|
| `src/agent/claude_code/pod.rs` | Add `setup-tools` init container; update main command to workspace path; add `--cli-path` arg; add headless env vars; update `PodBuildParams` |
| `src/agent/service.rs` | Pass `claude_cli_version` to `PodBuildParams` |
| `src/agent/claude_code/pod.rs` (reserved env vars) | Add `CLAUDE_CLI_VERSION`, `DISABLE_AUTOUPDATER`, `DISABLE_TELEMETRY`, `PATH` |
| `docker/Dockerfile.claude-runner` | Update to include agent-runner binary (optional — for pre-built image users) |
| `docker/entrypoint.sh` | Update to call agent-runner instead of claude directly |

### Test Outline — PR 2

**New behaviors to test:**
- `build_setup_tools_container` generates correct script with version interpolation — unit
- `build_setup_tools_container` has correct env vars (PLATFORM_API_TOKEN, PLATFORM_API_URL, CLAUDE_CLI_VERSION) — unit
- Init containers are ordered: git-clone → setup-tools → setup (optional) — unit
- Main container command is `/workspace/.platform/bin/agent-runner` — unit
- Agent-runner args include `--cli-path /workspace/.platform/bin/claude` — unit
- Env vars include `DISABLE_AUTOUPDATER=1` and `DISABLE_TELEMETRY=1` — unit
- PATH env var includes `/workspace/.platform/bin` — unit
- `CLAUDE_CLI_VERSION`, `DISABLE_AUTOUPDATER`, `DISABLE_TELEMETRY`, `PATH` are reserved — unit
- Setup script uses curl when available, node as fallback — unit (script content check)
- Setup script is idempotent (skips existing tools) — unit (script content check)

**Existing tests affected:**
- All tests in `src/agent/claude_code/pod.rs::tests` (~40 tests) — command path changes from `"agent-runner"` to `/workspace/.platform/bin/agent-runner`; args now include `--cli-path`; init containers now include `setup-tools`; env vars now include headless vars

**Estimated test count:** ~12 unit (new) + ~40 unit (updated existing)

### Verification

1. Build the Docker image: `just docker`
2. Verify agent-runner binaries exist at `/data/agent-runner/{amd64,arm64}` in the image
3. Launch a test agent session with the default image → setup-tools init container runs, downloads agent-runner, installs Claude CLI
4. Launch a test agent session with pre-built `platform-claude-runner` image → setup detects existing tools and skips
5. Verify `just test-unit` passes with all pod.rs test updates
6. Verify `just test-integration` passes with download endpoint tests

---

## Cross-cutting concerns

- [ ] Auth: download endpoint uses `AuthUser` extractor (Bearer token)
- [ ] No sensitive data in logs (setup script uses PLATFORM_API_TOKEN but doesn't echo it)
- [ ] Backward compatible: pre-built images still work (idempotent setup script)
- [ ] AppState unchanged — no test helper changes for AppState (only Config changes)
- [ ] Agent-runner binary is pure Rust (rustls) — no OpenSSL deps, cross-compilation is clean
- [ ] Reserved env vars updated to prevent override of new vars (CLAUDE_CLI_VERSION, DISABLE_AUTOUPDATER, PATH)

## Open questions / future work

1. **Air-gapped clusters**: If pods can't reach `registry.npmjs.org` or `claude.ai`, auto-setup fails for Claude CLI. Workaround: use pre-built image. Future: platform could proxy/cache Claude CLI binary.
2. **Native installer vs npm**: Currently using npm (more reliable, well-tested). Native installer could be used as fallback for images without Node.js.
3. **Setup latency**: npm install adds ~30-60s to pod startup. Acceptable for long-running agent sessions. Could be optimized with PVC caching in future.
4. **Multi-arch Docker builds**: Currently cross-compiles both arches in a single build. For large-scale CI, could use Docker buildx with QEMU or split into per-arch builds.
