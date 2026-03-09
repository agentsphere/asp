# Plan 42: DaemonSet Registry Proxy + Registry Image Seeding

## Context

The platform's built-in OCI registry needs two improvements:

1. **Containerd access** — Kind nodes need `containerdConfigPatches` to trust the HTTP registry. Fragile, won't work in prod. Replace with a DaemonSet socat proxy (`localhost:*` is implicitly trusted).
2. **Base image availability** — The `platform-runner` image must be in the registry before any agent session or project pipeline can run. Currently there's no mechanism for this. Instead of building on first startup (slow, requires Kaniko), **seed the registry from pre-built OCI tarballs on the filesystem**.

### Image lifecycle

1. **First startup**: Bootstrap seeds `platform-runner:latest` from an OCI tarball on disk → image available immediately
2. **Updates**: The `platform-runner` project exists with Dockerfiles in its git repo. Pushing changes triggers a pipeline rebuild via Kaniko, updating the registry image
3. **Dev/test**: `kind-up.sh` builds the tarball once. All subsequent test runs reuse it — no rebuild per start

---

## Part A: DaemonSet Registry Proxy

*(Unchanged from original plan — see sections A1–A8)*

### A1. Add `registry_node_url` to Config

**`src/config.rs`**: New field:
```rust
pub registry_node_url: Option<String>,  // PLATFORM_REGISTRY_NODE_URL
```
Falls back to `registry_url` if unset. Add to `test_default()`: `registry_node_url: None`.

### A2. Update agent service to use `registry_node_url`

**`src/agent/service.rs`**: Pass `registry_node_url.or(registry_url)` to:
- `create_pull_secret` — Docker config auth must match image ref hostname
- `PodBuildParams.registry_url` — image refs in pod specs that containerd pulls

### A3. Update pipeline executor

**`src/pipeline/executor.rs`**: Add `node_registry_url(config)` helper. Use it for:
- `detect_and_write_deployment()` — image refs in DB / pod specs
- `detect_and_publish_dev_image()` — dev image ref stored as `projects.agent_image`
- Keep `registry_url` for `build_env_vars()` (`REGISTRY` env var for Kaniko push from inside pods)
- `create_registry_secret()` — add `registry_node_url` entry to Docker config auths alongside `registry_url`

### A4. Remove containerdConfigPatches

**`hack/kind-config.yaml`**: Remove lines 3–6 (containerdConfigPatches).

### A5. DaemonSet creation in deploy-services.sh

**`hack/deploy-services.sh`**: Add registry proxy DaemonSet when `REGISTRY_BACKEND_HOST` + `REGISTRY_BACKEND_PORT` are set. Uses `alpine/socat:latest` with hostPort binding.

### A6. Update dev-env.sh and test-in-cluster.sh

Remove containerd certs.d patching. Pass env vars to deploy-services.sh. Export `PLATFORM_REGISTRY_NODE_URL`. Add DaemonSet cleanup.

### A7. Pre-load socat image in kind-up.sh

```bash
docker pull alpine/socat:latest 2>/dev/null || true
kind load docker-image alpine/socat:latest --name "$CLUSTER_NAME" 2>/dev/null || true
```

### A8. Update test helpers

Add `registry_node_url` to Config in `tests/helpers/mod.rs` and `tests/e2e_helpers/mod.rs`.

---

## Part B: Registry Image Seeding (replaces auto-trigger pipeline)

### How the registry stores images

| Layer | Storage | Format |
|---|---|---|
| Blobs (layers + config) | MinIO | `registry/blobs/sha256/{hex}` (content-addressable) |
| Blob metadata | `registry_blobs` table | digest, size_bytes, minio_path |
| Blob → repo links | `registry_blob_links` table | repository_id, blob_digest |
| Manifests | `registry_manifests` table | repository_id, digest, media_type, content (bytes) |
| Tags | `registry_tags` table | repository_id, name → manifest_digest |

### B1. New module: `src/registry/seed.rs`

**Purpose**: Parse OCI layout tarballs and import blobs/manifests/tags into the registry.

**New dependencies** in `Cargo.toml`:
- `tar = "0.4"` — pure Rust tar reader
- `flate2 = "1"` — gzip decompression (already transitive via tower-http)

**OCI layout tarball format** (from `docker buildx build --output type=oci`):
```
oci-layout           → {"imageLayoutVersion": "1.0.0"}
index.json           → {"manifests": [{"digest": "sha256:...", "mediaType": "...", "size": N}]}
blobs/sha256/{hex}   → content-addressable blobs (config, compressed layers, manifest)
```

**Types** (internal to seed.rs):
```rust
#[derive(Deserialize)]
struct OciLayout { image_layout_version: String }

#[derive(Deserialize)]
struct OciIndex { manifests: Vec<OciIndexEntry> }

#[derive(Deserialize)]
struct OciIndexEntry { media_type: String, digest: String, size: i64 }

pub enum SeedResult { AlreadyExists, Imported { manifest_digest: String, blob_count: usize } }
```

**Core function**: `seed_image(pool, minio, repository_id, tarball_path, tag) -> Result<SeedResult>`

Implementation steps:
1. **Idempotency check**: If tag already exists in `registry_tags`, return `AlreadyExists`
2. **Open tarball**: Detect gzip from extension, read with `tar::Archive`
3. **Extract entries**: Collect all files into `HashMap<String, Vec<u8>>` (normalize `./` prefix)
4. **Validate**: Check `oci-layout` version is `"1.0.0"`, `index.json` has manifests
5. **Import blobs**: For each `blobs/sha256/{hex}`:
   - Verify digest matches content via `sha256_digest()`
   - Write to MinIO at `registry/blobs/sha256/{hex}`
   - INSERT into `registry_blobs` (ON CONFLICT DO NOTHING)
   - INSERT into `registry_blob_links` (ON CONFLICT DO NOTHING)
6. **Import manifest**: Read manifest blob referenced by `index.json`:
   - INSERT into `registry_manifests`
   - INSERT into `registry_tags` (tag → manifest_digest)
7. **Handle image index**: If manifest is an image index (multi-arch), also import sub-manifests as separate `registry_manifests` rows

**Scanning function**: `seed_all(pool, minio, seed_path) -> Result<()>`
- Reads directory, finds `*.tar` / `*.tar.gz` files
- Filename stem = repository name (e.g. `platform-runner.tar` → repo `platform-runner`)
- Looks up `registry_repositories` by name, calls `seed_image()` for each
- Logs + skips if directory doesn't exist or repo not found

### B2. Config: `seed_images_path`

**`src/config.rs`**: New field:
```rust
pub seed_images_path: PathBuf,  // PLATFORM_SEED_IMAGES_PATH, default "/data/seed-images"
```

### B3. Integration in main.rs

After bootstrap, before starting the HTTP server:
```rust
// Seed registry images from OCI layout tarballs (idempotent)
if let Err(e) = registry::seed::seed_all(&pool, &state.minio, &cfg.seed_images_path).await {
    tracing::warn!(error = %e, "registry image seeding failed");
}
```

Runs on **every startup** (not just first boot). Idempotent — existing tags are skipped. Failure is a warning, not fatal.

### B4. Build seed images in kind-up.sh

**`hack/kind-up.sh`**: After cluster creation:
```bash
mkdir -p /tmp/platform-e2e/seed-images
docker buildx build \
  --file docker/Dockerfile.claude-runner-bare \
  --output "type=oci,dest=/tmp/platform-e2e/seed-images/platform-runner.tar" \
  .
```

Built **once** during cluster setup. Reused across all test runs via the shared `/tmp/platform-e2e` mount.

### B5. Export env var in scripts

**`hack/test-in-cluster.sh`** and **`hack/dev-env.sh`**:
```bash
export PLATFORM_SEED_IMAGES_PATH="/tmp/platform-e2e/seed-images"
```

### B6. Wire module

**`src/registry/mod.rs`**: Add `pub mod seed;`

### B7. Keep platform-runner project for updates

The existing `create_runner_project()` in `bootstrap.rs` still creates the project + `registry_repositories` entry. No changes needed — this gives the seed module a target repository.

Future work (not in this plan): Add Dockerfiles + `.platform.yaml` to the `platform-runner` git repo so pushing changes triggers a pipeline rebuild. The seed only handles the **initial** image; updates flow through the normal pipeline.

---

## Files Changed

| File | Change |
|---|---|
| `Cargo.toml` | Add `tar = "0.4"`, `flate2 = "1"` |
| `src/registry/seed.rs` | **New**: OCI layout parser + registry importer |
| `src/registry/mod.rs` | Add `pub mod seed;` |
| `src/config.rs` | Add `seed_images_path` + `registry_node_url` fields |
| `src/main.rs` | Call `seed_all()` after bootstrap |
| `src/agent/service.rs` | Use `registry_node_url` for image refs + pull secrets |
| `src/pipeline/executor.rs` | `node_registry_url()` helper, split registry URL usage |
| `hack/kind-config.yaml` | Remove `containerdConfigPatches` |
| `hack/kind-up.sh` | Pre-load socat, build runner OCI tarball |
| `hack/deploy-services.sh` | Add registry proxy DaemonSet |
| `hack/dev-env.sh` | Remove certs.d, add env vars, DaemonSet cleanup |
| `hack/test-in-cluster.sh` | Same as dev-env.sh |
| `tests/helpers/mod.rs` | Add `seed_images_path` + `registry_node_url` to Config |
| `tests/e2e_helpers/mod.rs` | Same |
| `Justfile` | Fix image path from `platform-runner/platform-runner` to `platform-runner` |

## Tests

**Unit tests** (in `src/registry/seed.rs`):
- `parse_oci_layout_valid` / `rejects_bad_version`
- `parse_index_json_valid` / `rejects_empty_manifests`
- `extract_tarball_entries` + path normalization
- `blob_digest_verification` (match/mismatch)
- `image_name_from_filename` extraction
- Build in-memory test tarballs with `tar::Builder`

**Integration test** (new `tests/registry_seed_integration.rs` or in existing registry tests):
- `seed_image_imports_blobs_and_manifest` — create test tarball, seed, verify DB + MinIO
- `seed_image_is_idempotent` — seed twice, second returns `AlreadyExists`
- `seed_all_scans_directory` — multiple tarballs, each imported

## Verification

1. `just cluster-down && just cluster-up` — recreates cluster, builds OCI tarball to `/tmp/platform-e2e/seed-images/`
2. `just test-unit` — seed module unit tests + config field tests pass
3. `just test-integration` — integration tests pass, `platform-runner:latest` available in registry after bootstrap
4. `just test-e2e` — E2E tests pass with DaemonSet proxy
5. `just dev` — dev environment starts, verify:
   - `kubectl get ds -n platform` shows `registry-proxy-*` ready
   - `curl -u admin:TOKEN http://localhost:$PORT/v2/platform-runner/tags/list` returns `{"tags":["latest"]}`
   - Agent sessions can pull `platform-runner:latest`
6. `just ci-full` — full CI passes (fmt + lint + deny + unit + integration + e2e + build)
