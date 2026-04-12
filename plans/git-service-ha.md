# HA Git Service — Longhorn RWX + Distributed Locking

Extract all Git operations from the monolithic platform API into a dedicated,
highly available microservice. The Git Service is the **sole owner** of
repository data on disk. API replicas become stateless proxies, forwarding git
operations via gRPC. Write serialization uses Valkey distributed locks, not
filesystem-level locking, eliminating NFS corruption risks entirely.

---

## Architecture Context

```
  External traffic (HTTP + SSH)
         │
         ▼
  ┌─────────────────────────────────────────────────┐
  │  Gateway DaemonSet (1 pod per node, hostPort)   │
  │  :80 → HTTP L7 proxy (HTTPRoute-based routing)  │
  │  :443 → HTTPS L7 proxy (TLS termination)        │
  │  :2222 → SSH L4 passthrough (TCP pipe)           │
  └──────┬────────────────────────┬─────────────────┘
         │ HTTP (L7 routed)       │ SSH (L4 passthrough)
         ▼                        ▼
   ┌──────────┐ ┌──────────┐   ┌──────────┐
   │ API (×N) │ │ API (×N) │   │ API :2222│  Stateless: Auth, RBAC, UI, API
   │  :8080   │ │  :8080   │   │ (russh)  │  SSH auth + branch protection
   └────┬─────┘ └────┬─────┘   └────┬─────┘
        │  gRPC      │              │ gRPC
        ▼            ▼              ▼
   ┌──────────┐ ┌──────────┐
   │ Git Svc  │ │ Git Svc  │  2+ replicas for HA
   │ Replica A│ │ Replica B│  Runs git commands
   └────┬─────┘ └────┬─────┘
        │            │
        ▼            ▼
   ┌──────────────────────┐
   │  Longhorn RWX PVC    │  /data/git-repos
   │  (3-node block repl) │  mounted to Git Svc pods only
   └──────────────────────┘

  Write lock: Valkey SETNX per project_id (TTL-based, auto-expire on crash)
  Read lock:  None — concurrent reads safe
  Internal LB: K8s Service (L4 round-robin via kube-proxy/IPVS)
```

---

## Current state (what moves)

### Project repos — currently in API process

| File | Functions | Operation type | Frequency |
|---|---|---|---|
| `src/git/smart_http.rs` | `info_refs()`, `upload_pack()`, `receive_pack()` | Smart HTTP protocol (streaming) | Every git clone/push |
| `src/git/ssh_server.rs` | `exec_request()`, `data()`, `pipe_git_to_ssh()` | SSH protocol (streaming) | Every SSH git op |
| `src/git/browser.rs` | `git_ls_tree()`, `git_show_blob()`, `git_log()`, `git_diff()`, etc. | Read-only file/commit browsing | Per UI request |
| `src/git/repo.rs` | `init_bare_repo()`, `init_bare_repo_with_files()` | Repo initialization | Project creation |
| `src/git/hooks.rs` | `post_receive()` | Post-push webhook/pipeline triggers | Per push |
| `src/git/protection.rs` | `is_force_push()` | Check if push is force-push | Per push |
| `src/api/merge_requests.rs` | `git_merge_no_ff()`, `git_squash_merge()`, `git_rebase_merge()` | MR merge (worktree-based) | Per merge |
| `src/pipeline/trigger.rs` | `read_file_at_ref()`, `read_dir_at_ref()`, `create_annotated_tag()`, `auto_bump_version()` | Pipeline reads + version tagging | Per push event |

### Ops repos — currently in deployer process

| File | Functions | Operation type |
|---|---|---|
| `src/deployer/ops_repo.rs` | `init_ops_repo()`, `commit_values()`, `write_file_to_repo()`, `sync_from_project_repo()`, `merge_branch()`, `revert_last_commit()` | Write (locked via `REPO_LOCKS`) |
| `src/deployer/ops_repo.rs` | `read_file_at_ref()`, `read_values()`, `get_head_sha()`, `get_branch_sha()`, `compare_branches()` | Read (no lock) |

### Who accesses what

| Consumer | Repo type | Access | After extraction |
|---|---|---|---|
| API replicas (smart HTTP) | Project | Read + write (push) | Proxy to Git Service |
| API replicas (SSH server) | Project | Read + write (push) | Proxy to Git Service |
| API replicas (browser) | Project | Read-only | Proxy to Git Service |
| API replicas (MR merge) | Project | Write | Proxy to Git Service |
| Pipeline executor | Project | Read-only (.platform.yaml, VERSION) | Call Git Service |
| Pipeline trigger | Project | Read + write (tags, version bump) | Call Git Service |
| Deployer reconciler | Ops | Read + write | Call Git Service (or keep local — see below) |

---

## Storage layer: Longhorn RWX

### How it works

Longhorn aggregates local VM storage (SSDs) across cluster nodes into
distributed block volumes. For RWX:

1. Longhorn creates a replicated block volume (3 replicas across nodes).
2. A **ShareManager** pod exposes the volume via NFS to multiple consumers.
3. Git Service pods mount the RWX PVC at `/data/git-repos`.

### Why RWX over RWO

With 2+ Git Service replicas for HA, all replicas need access to the same
repos. RWO would limit us to 1 replica (or require sticky routing — defeating
HA). RWX lets any replica serve any request.

### NFS performance concern

NFS adds latency to `stat()` calls. Git makes many small random reads
(loose objects, pack index lookups). Mitigation:

1. **Git repacking** — a background maintenance task (see section 7) runs
   `git repack -a -d` + `git pack-refs --all` to consolidate loose objects
   and refs into packfiles. This drastically reduces `stat()` calls per
   operation: one packfile index lookup vs hundreds of loose object reads.

2. **NFS attribute caching** — Longhorn's ShareManager supports `actimeo`
   tuning. Since the Git Service is the sole writer, we can safely cache
   attributes for longer periods.

3. **Profile first** — NFS latency may be perfectly fine at our scale (tens
   of repos, not thousands). Don't over-optimize before measuring.

---

## The concurrency shield: Valkey distributed locks

### The problem

Git's native locking (`refs/heads/main.lock`, packfile locks) uses `fcntl` or
lock files. These work on local filesystems but **fail silently or corrupt data
over NFS** when two processes on different nodes write simultaneously.

The in-process `REPO_LOCKS` mutex in `src/deployer/ops_repo.rs` serializes
writes within one process. With 2+ Git Service replicas, it's useless.

### The solution: Valkey SETNX with TTL

```rust
const LOCK_TTL_SECS: u64 = 120;  // max time a write can hold the lock
const LOCK_WAIT_MS: u64 = 100;   // poll interval when waiting
const LOCK_MAX_WAIT_SECS: u64 = 30;  // give up after this

/// Acquire a distributed write lock for a repository.
/// Returns a guard that releases the lock on drop.
async fn acquire_repo_lock(
    valkey: &fred::clients::Pool,
    repo_id: &str,  // project UUID or ops repo name
) -> Result<RepoLockGuard, GitServiceError> {
    let key = format!("gitlock:{repo_id}");
    let lock_value = uuid::Uuid::new_v4().to_string();  // unique per acquisition
    let deadline = Instant::now() + Duration::from_secs(LOCK_MAX_WAIT_SECS);

    loop {
        // SET key value NX EX ttl — atomic acquire
        let acquired: bool = valkey.set(
            &key, &lock_value,
            Some(Expiration::EX(LOCK_TTL_SECS as i64)),
            Some(SetPolicy::NX),
            false,
        ).await?;

        if acquired {
            return Ok(RepoLockGuard {
                valkey: valkey.clone(),
                key,
                value: lock_value,
                released: false,
            });
        }

        if Instant::now() > deadline {
            return Err(GitServiceError::LockTimeout(repo_id.to_string()));
        }
        tokio::time::sleep(Duration::from_millis(LOCK_WAIT_MS)).await;
    }
}

/// Compare-and-delete Lua script: only release if we still hold the lock.
const RELEASE_SCRIPT: &str = r#"
    if redis.call('get', KEYS[1]) == ARGV[1] then
        return redis.call('del', KEYS[1])
    end
    return 0
"#;

/// RAII guard — explicit async release preferred, Drop as safety net.
struct RepoLockGuard {
    valkey: fred::clients::Pool,
    key: String,
    value: String,
    released: bool,
}

impl RepoLockGuard {
    /// Explicit async release — call this in handlers before returning.
    /// Preferred over Drop because it awaits the Valkey round-trip and
    /// logs errors. Returns true if the lock was successfully released.
    async fn release(mut self) -> bool {
        self.released = true;
        let result: Result<i32, _> = self.valkey
            .eval(RELEASE_SCRIPT, vec![&self.key], vec![&self.value])
            .await;
        match result {
            Ok(1) => true,
            Ok(_) => {
                tracing::warn!(key = %self.key, "lock already expired or stolen");
                false
            }
            Err(e) => {
                tracing::error!(key = %self.key, error = %e, "failed to release lock");
                false
            }
        }
    }
}

impl Drop for RepoLockGuard {
    fn drop(&mut self) {
        if self.released {
            return;  // already released via .release().await
        }
        // Safety net: fire-and-forget release if handler didn't call .release()
        tracing::warn!(key = %self.key, "lock released via Drop (prefer .release().await)");
        let valkey = self.valkey.clone();
        let key = self.key.clone();
        let value = self.value.clone();
        tokio::spawn(async move {
            let _ = valkey.eval::<(), _, _>(RELEASE_SCRIPT, vec![key], vec![value]).await;
        });
    }
}
```

### Lock rules

| Operation | Lock required? | Key |
|---|---|---|
| `git upload-pack` (clone/fetch) | **No** | — |
| `git receive-pack` (push) | **Yes** | `gitlock:{project_id}` |
| MR merge (no-ff, squash, rebase) | **Yes** | `gitlock:{project_id}` |
| `create_annotated_tag` | **Yes** | `gitlock:{project_id}` |
| `auto_bump_version` | **Yes** | `gitlock:{project_id}` |
| `init_bare_repo` | **No** | New repo, no contention |
| File browser reads | **No** | — |
| Pipeline `read_file_at_ref` | **No** | — |
| Ops repo writes | **Yes** | `gitlock:ops:{ops_repo_name}` |
| Ops repo reads | **No** | — |

### Failover: crash mid-push

1. Git Service Replica B holds lock for `project_id: 123`, crashes mid-push.
2. Client connection drops. Developer retries.
3. API proxies retry to Git Service Replica A.
4. Lock TTL expires (120s). Replica A acquires lock.
5. Git ref state is consistent — `receive-pack` writes are atomic at the ref
   level (either the ref updates or it doesn't). Partial packfile writes leave
   orphan objects but don't corrupt refs.
6. Push succeeds. Zero data corruption.

---

## Git Service API

The Git Service exposes a gRPC (or HTTP/2) API. Two categories:

### Streaming operations (git protocol)

These proxy the git wire protocol between client and server. The API replica
acts as a transparent pipe — it handles auth/RBAC, then streams bytes
bidirectionally.

```protobuf
service GitProtocol {
  // Smart HTTP: client sends request body, gets response body
  rpc InfoRefs(InfoRefsRequest) returns (InfoRefsResponse);
  rpc UploadPack(stream DataChunk) returns (stream DataChunk);
  rpc ReceivePack(stream DataChunk) returns (ReceivePackResponse);

  // SSH: bidirectional stream
  rpc SshSession(stream DataChunk) returns (stream DataChunk);
}

message InfoRefsRequest {
  string project_id = 1;
  string service = 2;     // "upload-pack" or "receive-pack"
}

message DataChunk {
  bytes data = 1;
}

message ReceivePackResponse {
  bytes data = 1;              // git protocol output
  repeated RefUpdate updates = 2;  // parsed ref updates for post-push hooks
}
```

**Why stream, not buffer:** A `git push` of 500MB (LFS or large repo) must not
be buffered in memory. Both the current smart HTTP and SSH implementations
already stream — the gRPC interface preserves this.

### Request-response operations (everything else)

```protobuf
service GitOps {
  // Repo lifecycle
  rpc InitBareRepo(InitRequest) returns (InitResponse);
  rpc InitBareRepoWithFiles(InitWithFilesRequest) returns (InitResponse);

  // File browser
  rpc LsTree(LsTreeRequest) returns (LsTreeResponse);
  rpc ShowBlob(ShowBlobRequest) returns (ShowBlobResponse);
  rpc Log(LogRequest) returns (LogResponse);
  rpc Diff(DiffRequest) returns (DiffResponse);
  rpc CommitInfo(CommitInfoRequest) returns (CommitInfoResponse);

  // Merge operations
  rpc MergeNoFF(MergeRequest) returns (MergeResponse);
  rpc SquashMerge(MergeRequest) returns (MergeResponse);
  rpc RebaseMerge(MergeRequest) returns (MergeResponse);

  // Pipeline support
  rpc ReadFileAtRef(ReadFileRequest) returns (ReadFileResponse);
  rpc ReadDirAtRef(ReadDirRequest) returns (ReadDirResponse);
  rpc CreateAnnotatedTag(TagRequest) returns (TagResponse);
  rpc AutoBumpVersion(BumpVersionRequest) returns (BumpVersionResponse);

  // Ops repo operations
  rpc InitOpsRepo(InitOpsRepoRequest) returns (InitResponse);
  rpc CommitValues(CommitValuesRequest) returns (CommitResponse);
  rpc WriteFileToRepo(WriteFileRequest) returns (CommitResponse);
  rpc SyncFromProjectRepo(SyncRequest) returns (CommitResponse);
  rpc MergeBranch(MergeBranchRequest) returns (CommitResponse);
  rpc RevertLastCommit(RevertRequest) returns (CommitResponse);
  rpc ReadValues(ReadValuesRequest) returns (ReadValuesResponse);
  rpc GetHeadSha(ShaRequest) returns (ShaResponse);
  rpc GetBranchSha(BranchShaRequest) returns (ShaResponse);
  rpc CompareBranches(CompareRequest) returns (CompareResponse);

  // Branch protection
  rpc IsForcePush(ForcePushRequest) returns (ForcePushResponse);

  // Maintenance
  rpc RepackRepo(RepackRequest) returns (RepackResponse);
}
```

### Implementation: thin wrappers

The Git Service handlers are thin wrappers around the existing functions. The
git logic doesn't change — it just runs behind a network boundary:

```rust
// In the Git Service binary:
async fn handle_merge_no_ff(req: MergeRequest) -> Result<MergeResponse> {
    let lock = acquire_repo_lock(&valkey, &req.project_id).await?;
    let result = git_merge_no_ff(
        &Path::new(&req.repo_path),
        &req.source_branch,
        &req.target_branch,
        &req.message,
        &req.author_name,
        &req.author_email,
    ).await;
    lock.release().await;  // explicit async release (Drop is safety net only)
    result.map(|sha| MergeResponse { commit_sha: sha })
}
```

---

## Request flows

### Write flow: `git push`

```
1. User pushes → hits API Replica A (smart HTTP or SSH)
2. API authenticates (Basic Auth / SSH key), resolves project, checks RBAC
3. API opens gRPC stream to Git Service (any replica via K8s Service)
4. Git Service Replica B receives stream
5. Replica B acquires Valkey lock: SETNX gitlock:{project_id} {uuid} EX 120
6. Lock acquired → Replica B spawns `git receive-pack` against /data/git-repos
7. Streams stdin/stdout between gRPC and git process
8. git receive-pack completes → Replica B parses ref updates from response
9. Replica B releases lock: compare-and-delete via Lua script
10. Returns ref updates to API Replica A
11. API Replica A runs post-push hooks:
    - fire_webhooks()
    - pipeline trigger (via Valkey event)
    - branch protection was already checked in step 2 (or by Git Service)
```

### Read flow: `git clone`

```
1. User clones → hits API Replica C
2. API authenticates, resolves project, checks read access
3. API opens gRPC stream to Git Service (any replica)
4. Git Service Replica A (no lock needed)
5. Spawns `git upload-pack` against /data/git-repos
6. Streams pack data back to API → to user
```

### Read flow: file browser

```
1. User views file → API Replica B handles /api/projects/{id}/files/{path}
2. API authenticates, checks read access
3. API calls Git Service: ShowBlob(project_id, ref, path)
4. Git Service runs: git show {ref}:{path}
5. Returns blob content to API
```

### Merge flow: MR merge

```
1. User clicks merge → API Replica A handles POST /api/projects/{id}/mrs/{n}/merge
2. API authenticates, checks write access, validates MR state
3. API calls Git Service: MergeNoFF(project_id, source, target, message, author)
4. Git Service Replica B acquires lock, creates worktree, runs merge
5. Returns merge commit SHA
6. API updates MR status in DB, fires webhooks, triggers pipeline
```

### Failover: crash mid-push

```
1. Git Service Replica B holds lock for project 123, crashes during receive-pack
2. Client connection drops (gRPC stream error)
3. Developer retries: git push
4. API proxies to Git Service Replica A
5. Replica A tries lock: SETNX gitlock:123 — blocked (Replica B still holds it)
6. Replica A polls every 100ms...
7. After TTL (≤120s), lock expires automatically
8. Replica A acquires lock, receive-pack succeeds
9. Git state is consistent: refs are atomic, partial packfiles are orphaned garbage
```

---

## Deployment

### Git Service

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: platform-git
spec:
  replicas: 2
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxUnavailable: 1    # always at least 1 replica serving
  template:
    spec:
      containers:
      - name: git
        image: platform:latest
        command: ["/usr/local/bin/platform-git"]
        ports:
        - containerPort: 9100   # gRPC
        - containerPort: 9090   # health probe
        volumeMounts:
        - name: git-repos
          mountPath: /data/git-repos
        livenessProbe:
          httpGet: { path: /healthz, port: 9090 }
          periodSeconds: 10
        readinessProbe:
          httpGet: { path: /readyz, port: 9090 }
          periodSeconds: 5
        resources:
          requests: { cpu: 250m, memory: 256Mi }
          limits: { cpu: "2", memory: 1Gi }
      volumes:
      - name: git-repos
        persistentVolumeClaim:
          claimName: platform-git-repos

---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: platform-git-repos
spec:
  accessModes: [ReadWriteMany]
  storageClassName: longhorn-rwx
  resources:
    requests:
      storage: 50Gi

---
apiVersion: v1
kind: Service
metadata:
  name: platform-git
spec:
  ports:
  - name: grpc
    port: 9100
    targetPort: 9100
  selector:
    app: platform-git
```

### API Deployment (updated)

```yaml
# No volumeMounts for git repos — API is stateless
apiVersion: apps/v1
kind: Deployment
metadata:
  name: platform-api
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: api
        image: platform:latest
        command: ["/usr/local/bin/platform"]
        env:
        - name: PLATFORM_GIT_SERVICE_URL
          value: "http://platform-git:9100"
```

### Deployer (ops repos)

Two options:

**Option A: Deployer uses Git Service for ops repos too.**
Deployer becomes fully stateless — calls Git Service for all ops repo reads and
writes. Clean separation.

**Option B: Deployer keeps its own RWO PVC for ops repos.**
Deployer is already single-replica (`FOR UPDATE SKIP LOCKED`). Ops repos don't
need multi-writer access. Simpler — no gRPC for ops repo operations.

**Recommendation: Option A for full consistency.** All git operations go through
one service. The deployer's existing `REPO_LOCKS` mutex is replaced by the same
Valkey lock the Git Service uses. One locking strategy, one storage location.

---

## Ingress: Gateway DaemonSet with HTTP + SSH

### Problem

Users need to `git clone`/`push` from their local machines via both HTTPS and
SSH. The SSH git protocol is raw TCP (not HTTP), so it needs L4 passthrough,
not L7 routing. The current gateway is an HTTP-only Deployment.

### Solution: DaemonSet + TCP passthrough

Convert the gateway from `Deployment` to `DaemonSet` and add a TCP passthrough
listener for SSH. One gateway pod per node, bound to `hostPort`:

```
User: git clone ssh://git@platform.example.com:2222/owner/repo.git

  → Node IP:2222 (hostPort on gateway DaemonSet)
  → Gateway pod: TCP passthrough (bidirectional tokio::io::copy)
  → K8s Service: platform-api:2222
  → API pod (russh: SSH key auth, RBAC, branch protection)
  → Git Service:9100 (gRPC: actual git process on disk)

User: git clone https://platform.example.com/owner/repo.git

  → Node IP:443 (hostPort on gateway DaemonSet)
  → Gateway pod: HTTP L7 proxy (HTTPRoute-based routing)
  → K8s Service: platform-api:8080
  → API pod (Basic Auth, RBAC, smart HTTP handler)
  → Git Service:9100 (gRPC: actual git process on disk)
```

### Gateway DaemonSet manifest

```yaml
apiVersion: apps/v1
kind: DaemonSet                          # was: Deployment
metadata:
  name: platform-gateway
spec:
  updateStrategy:
    type: RollingUpdate
    rollingUpdate:
      maxUnavailable: 1                  # always some nodes serving
  template:
    spec:
      serviceAccountName: platform-gateway
      containers:
      - name: gateway
        image: platform-proxy:v1
        args: ["--gateway"]
        ports:
        - containerPort: 8080
          hostPort: 80                   # HTTP ingress
          name: http
        - containerPort: 8443
          hostPort: 443                  # HTTPS ingress
          name: https
        - containerPort: 2222
          hostPort: 2222                 # SSH git ingress (TCP passthrough)
          name: ssh
        - containerPort: 15020
          name: health
        env:
        - name: PROXY_GATEWAY_HTTP_PORT
          value: "8080"
        - name: PROXY_GATEWAY_TLS_PORT
          value: "8443"
        - name: PROXY_GATEWAY_TCP_FORWARDS
          value: "2222:platform-api.platform.svc:2222"
        readinessProbe:
          httpGet: { path: /readyz, port: 15020 }
          periodSeconds: 5
        livenessProbe:
          httpGet: { path: /healthz, port: 15020 }
          periodSeconds: 10
```

No `NodePort` Service needed — `hostPort` binds directly on node IPs. External
load balancer (or DNS round-robin) points to node IPs.

### TCP passthrough implementation

~50 lines in the gateway binary. No HTTP parsing, no `russh` dependency — just
a bidirectional TCP pipe:

```rust
/// TCP passthrough listener for non-HTTP protocols (SSH git).
/// Configured via PROXY_GATEWAY_TCP_FORWARDS env var.
///
/// Format: "listen_port:backend_host:backend_port[,...]"
/// Example: "2222:platform-api.platform.svc:2222"
async fn run_tcp_passthrough(
    listen_port: u16,
    backend_addr: String,    // e.g. "platform-api.platform.svc:2222"
    mut shutdown: watch::Receiver<()>,
) {
    let addr = SocketAddr::from(([0, 0, 0, 0], listen_port));
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(port = listen_port, error = %e, "TCP passthrough bind failed");
            return;
        }
    };
    tracing::info!(port = listen_port, backend = %backend_addr, "TCP passthrough listening");

    loop {
        tokio::select! {
            accept = listener.accept() => {
                let Ok((client_stream, peer)) = accept else { continue };
                let backend = backend_addr.clone();
                tokio::spawn(async move {
                    let server_stream = match TcpStream::connect(&backend).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::debug!(peer = %peer, error = %e, "TCP backend connect failed");
                            return;
                        }
                    };
                    let (mut cr, mut cw) = client_stream.into_split();
                    let (mut sr, mut sw) = server_stream.into_split();
                    // Bidirectional pipe — closes when either side hangs up
                    tokio::select! {
                        r = tokio::io::copy(&mut cr, &mut sw) => {
                            if let Err(e) = r { tracing::debug!(error = %e, "client→server copy"); }
                        }
                        r = tokio::io::copy(&mut sr, &mut cw) => {
                            if let Err(e) = r { tracing::debug!(error = %e, "server→client copy"); }
                        }
                    }
                });
            }
            _ = shutdown.changed() => break,
        }
    }
}
```

### What changes in the gateway reconciler

The `src/gateway/mod.rs` reconciler currently creates a `Deployment` +
`NodePort` `Service`. Changes needed:

| Current | After |
|---|---|
| `Deployment` (replicas: 1) | `DaemonSet` |
| `NodePort` Service | Remove (hostPort replaces it) |
| Ports: HTTP, HTTPS | Ports: HTTP, HTTPS, SSH (TCP passthrough) |
| Container args: `["--gateway"]` | Same (TCP forwards configured via env) |

New config env vars for the reconciler:

| Env var | Default | Purpose |
|---|---|---|
| `PLATFORM_GATEWAY_SSH_PORT` | `2222` | SSH hostPort on each node |
| `PLATFORM_GATEWAY_TCP_FORWARDS` | (empty) | TCP passthrough config for the gateway container |

New config env vars for the gateway binary:

| Env var | Default | Purpose |
|---|---|---|
| `PROXY_GATEWAY_TCP_FORWARDS` | (empty) | Comma-separated `port:host:port` TCP forward rules |

### Why SSH stays in the API, not the gateway

The gateway is a **dumb TCP pipe** for SSH — no `russh`, no crypto, no auth.
This is deliberate:

1. **Auth centralization** — SSH key lookup, user resolution, RBAC permission
   checks, and branch protection all live in the API. Moving them to the
   gateway would duplicate auth logic or require the gateway to call back
   to the API anyway.

2. **Minimal gateway deps** — The gateway binary is small (~2MB, no DB/Valkey
   deps). Adding `russh` + SSH key management would bloat it and create a
   second auth surface.

3. **Consistent auth path** — Both HTTPS and SSH git requests arrive at the
   API for auth. The only difference is transport: HTTP Basic Auth vs SSH
   public key. After auth, both proxy to Git Service identically via gRPC.

### External load balancer

With `hostPort`, each node exposes :80, :443, and :2222 directly. The external
LB configuration depends on the environment:

```
# DNS round-robin (simplest, no dedicated LB)
platform.example.com → [node1-ip, node2-ip, node3-ip]

# Or: cloud/hardware LB
LB :80   → node1:80, node2:80, node3:80     (HTTP)
LB :443  → node1:443, node2:443, node3:443  (HTTPS, TLS passthrough or termination)
LB :2222 → node1:2222, node2:2222, node3:2222 (SSH, TCP mode)
```

The LB should use TCP mode for the SSH port (not HTTP health checks). Health
check: TCP connect to port 2222 (gateway TCP listener accepts immediately).

---

## Repo maintenance: background repacking

NFS performance degrades with many loose objects (each `stat()` hits the
network). The Git Service runs a periodic maintenance task:

```rust
async fn repo_maintenance_loop(
    repos_path: PathBuf,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(3600));  // hourly

    loop {
        tokio::select! {
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                let entries = std::fs::read_dir(&repos_path).ok();
                // Walk all owner dirs → repo dirs
                for owner_dir in entries.into_iter().flatten().filter_map(|e| e.ok()) {
                    let repos = std::fs::read_dir(owner_dir.path()).ok();
                    for repo_dir in repos.into_iter().flatten().filter_map(|e| e.ok()) {
                        let path = repo_dir.path();
                        if !path.extension().is_some_and(|e| e == "git") {
                            continue;
                        }

                        // Don't repack while a write is in progress
                        // (try-lock: skip if locked, don't wait)
                        let repo_name = path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown");
                        // Best-effort: skip repos currently being written to
                        tracing::debug!(repo = %path.display(), "repacking");

                        // Repack: merge loose objects into packfiles
                        let _ = tokio::process::Command::new("git")
                            .args(["-C", &path.to_string_lossy(),
                                   "repack", "-a", "-d", "--quiet"])
                            .status().await;

                        // Pack refs: consolidate loose refs
                        let _ = tokio::process::Command::new("git")
                            .args(["-C", &path.to_string_lossy(),
                                   "pack-refs", "--all"])
                            .status().await;

                        // Prune unreachable objects older than 2 weeks
                        let _ = tokio::process::Command::new("git")
                            .args(["-C", &path.to_string_lossy(),
                                   "prune", "--expire=2.weeks.ago"])
                            .status().await;

                        // Yield between repos to avoid starving other tasks
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }
}
```

**Only one replica should run maintenance** — use the same Valkey lock pattern
(or a separate `gitlock:maintenance` key) to ensure only one replica repacks
at a time.

---

## What changes where (implementation)

### New crate: `crates/platform-git/`

```
crates/platform-git/
├── Cargo.toml          # tonic, prost, tokio, fred
├── build.rs            # tonic-build protobuf compilation
├── proto/
│   └── git.proto       # service definition
└── src/
    ├── main.rs         # startup: gRPC server + maintenance loop + health
    ├── protocol.rs     # SmartHTTP/SSH streaming handlers (wraps existing git spawning)
    ├── ops.rs          # request-response handlers (browser, merge, tag, etc.)
    ├── lock.rs         # acquire_repo_lock(), RepoLockGuard
    ├── maintenance.rs  # repo_maintenance_loop()
    └── health.rs       # /healthz, /readyz on :9090
```

### Existing code changes

| File | Change |
|---|---|
| `src/git/smart_http.rs` | Replace `Command::new("git")` with gRPC call to Git Service. `run_git_service()` becomes `proxy_to_git_service()` — forward body stream, return response stream. |
| `src/git/ssh_server.rs` | `exec_request()` opens gRPC stream to Git Service instead of spawning local git. Bidirectional pipe: SSH channel ↔ gRPC stream ↔ git process. |
| `src/git/browser.rs` | All handlers call Git Service `GitOps` RPCs instead of local git commands. |
| `src/git/repo.rs` | `init_bare_repo()` → calls Git Service `InitBareRepo`. |
| `src/api/merge_requests.rs` | `git_merge_no_ff()` etc → call Git Service `MergeNoFF` etc. |
| `src/pipeline/trigger.rs` | `read_file_at_ref()`, `create_annotated_tag()`, `auto_bump_version()` → Git Service RPCs. |
| `src/deployer/ops_repo.rs` | All operations → Git Service RPCs. Remove `REPO_LOCKS`. |
| `src/config.rs` | Add `PLATFORM_GIT_SERVICE_URL` (default `http://platform-git:9100`). |
| `src/store.rs` | Remove `git_repos_path` from AppState (API no longer needs it). |

### gRPC with tonic (decided)

**Use tonic (gRPC).** The deciding factor is `receive-pack`: it's genuinely
bidirectional — the client sends a packfile while the server simultaneously
sends progress/error messages back on the same stream. HTTP/1.1 can't do this
at all. Axum with HTTP/2 *could* but requires manual framing. Tonic gives
native bidirectional streaming with codegen and type safety.

- `tonic` + `prost` + `prost-build` added to `crates/platform-git/`
- Protobuf service definition generates both server (Git Service) and client
  (API proxy) code
- `upload-pack` is server-streaming (client sends request, server streams pack)
- `receive-pack` is bidirectional-streaming (pack data up, progress/refs down)
- Request-response ops (browser, merge, tag) are simple unary RPCs

---

## Migration path

### Phase 1: Distributed lock (in-place, no extraction)

Add Valkey-based `acquire_repo_lock()` to the current monolith. Replace
`REPO_LOCKS` in ops_repo.rs. Add locking to MR merge operations (currently
unlocked). This fixes the multi-replica write safety issue immediately,
regardless of storage backend.

**Files:** `src/deployer/ops_repo.rs`, `src/api/merge_requests.rs`,
`src/pipeline/trigger.rs` — add `acquire_repo_lock()` before writes.

### Phase 2: Extract Git Service binary

1. Create `crates/platform-git/` with API definition + handlers.
2. Move git execution logic (spawn git process, stream I/O) into the new crate.
3. Keep existing functions in `src/git/` but rewrite internals to call Git
   Service instead of local git.
4. Both paths work: local git (dev mode) or Git Service (production).
5. Feature flag: `PLATFORM_GIT_SERVICE_URL` — if set, proxy to service;
   if empty, use local disk (backward compatible for dev).

### Phase 3: Longhorn RWX PVC + deploy

1. Provision Longhorn RWX PVC.
2. Deploy Git Service with 2 replicas mounting the PVC.
3. Migrate existing repos from API pod's PVC to the shared PVC.
4. Update API deployment: remove git PVC mount, set `PLATFORM_GIT_SERVICE_URL`.
5. Update executor/deployer: set `PLATFORM_GIT_SERVICE_URL`.

### Phase 4: Ops repo migration

1. Move ops repo storage to the same RWX PVC (under `/data/git-repos/ops/`).
2. Deployer calls Git Service for ops repo operations.
3. Remove deployer's ops repo PVC.

### Phase 5: Enable maintenance + monitoring

1. Enable repo_maintenance_loop in Git Service.
2. Add metrics: lock acquisition time, lock wait count, lock timeout count,
   git command duration, repack duration.
3. Alert on lock timeouts > threshold.

---

## Comparison with alternatives

| | RWX + advisory locks (no service) | Git Service + Longhorn RWX | MinIO pull-operate-push |
|---|---|---|---|
| Architecture | Shared PVC across API pods | Dedicated service owns storage | Object store + caching layer |
| API pods stateless? | No (mount PVC) | **Yes** | Yes |
| Write safety | Postgres advisory locks | **Valkey distributed locks** | Valkey locks + upload |
| Read performance | NFS latency on every op | NFS latency (but only Git Service) | Cache hit fast, miss = download |
| Fits service decoupling | Partially | **Fully** | Fully |
| Complexity | Low | Medium | High |
| HA failover | Lock TTL in Postgres | **Lock TTL in Valkey (faster)** | Lock TTL + cache invalidation |
| Repo size limit | PVC size | PVC size | Object store (unlimited) |
| Cold start | Instant (PVC mounted) | Instant (PVC mounted) | Download all repos |
| New crate? | No | Yes (~500 lines) | Yes (~1000+ lines) |
| NFS corruption risk | Possible without locks | **Eliminated by app-level locks** | N/A (no NFS) |

---

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| **NFS performance** — git over NFS slower than local | Repacking (hourly), NFS attribute caching, profile before optimizing. Longhorn local-path fallback if NFS is a bottleneck. |
| **Lock TTL too short** — large push takes > 120s | Make TTL configurable (`PLATFORM_GIT_LOCK_TTL_SECS`). Monitor lock durations. |
| **Lock TTL too long** — crashed replica blocks writes for 2 min | Acceptable for git pushes (user retries). Could reduce to 60s for ops repos (smaller writes). |
| **Longhorn ShareManager SPOF** — NFS server pod crashes | Longhorn auto-restarts ShareManager. Brief I/O pause (seconds), not data loss. Git clients retry. |
| **gRPC/HTTP overhead** — extra network hop for every git op | Microseconds for request/response ops. Streaming ops: zero-copy pipe, same throughput as local. |
| **Migration: repo data move** — copy repos from old PVC to new | Use `rsync` or `git clone --mirror`. Can do live (read from old, write to new) during transition. |
| **Dev mode complexity** — developers don't want to run Git Service locally | Feature flag: `PLATFORM_GIT_SERVICE_URL` unset → use local disk. Same as today for `just run`. |

---

## Local dev experience

```bash
# Default: no Git Service needed. API uses local disk.
just run              # works as today — git repos on local disk

# Full HA mode (for testing service decoupling):
just run-git          # starts Git Service on :9100
just run              # API connects to Git Service via PLATFORM_GIT_SERVICE_URL
```

When `PLATFORM_GIT_SERVICE_URL` is unset, the API falls back to local git
operations (current behavior). This keeps the dev experience simple — no
extra process needed for basic development.

---

## Resolved design decisions

1. **gRPC with tonic** — `receive-pack` is genuinely bidirectional (packfile
   up + progress down simultaneously). HTTP can't do this cleanly. See "gRPC
   with tonic" section above.

2. **SSH server stays in API** — SSH server (`russh`) accepts TCP connections,
   authenticates, then proxies git I/O to Git Service via gRPC stream. Auth
   stays centralized in the API.

3. **Branch protection stays in API** — API parses ref updates before piping
   to `receive-pack` and rejects protected-branch pushes. Git Service is a
   dumb executor with no auth/RBAC knowledge.

4. **Post-push hooks stay in API** — Git Service returns parsed ref updates
   in the `ReceivePackResponse`. API runs webhooks/pipeline triggers. Git
   Service has no DB access.

5. **K8s Service for internal LB** — The platform gateway proxy (HTTPRoute
   ingress controller) is HTTP/1.1 only, buffers entire responses, and is
   designed for external ingress. For internal API→Git Service traffic, the
   plain K8s `Service` (`platform-git`) provides L4 round-robin across healthy
   pods via kube-proxy/IPVS. Combined with the Git Service's readiness probes,
   this is sufficient. No custom LB needed.

6. **Lock release: explicit async, Drop as fallback** — Handlers call
   `lock.release().await` for clean release with error logging. `Drop` fires
   a `tokio::spawn` safety net only if the handler panics or forgets.
