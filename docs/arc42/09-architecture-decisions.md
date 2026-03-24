# 9. Architecture Decisions

## ADR-001: Single Binary over Microservices

**Status**: Accepted

**Context**: The platform replaces 8+ separate tools. A microservices approach would require service discovery, inter-service auth, distributed transactions, and independent deployment pipelines for each service.

**Decision**: Build a single Rust binary containing all 11 modules. Modules communicate through shared `AppState` in-process.

**Consequences**:
- (+) Zero IPC overhead — function calls between modules
- (+) Single container image, one K8s Deployment, atomic deploys
- (+) Shared connection pools (Postgres, Valkey, MinIO)
- (+) Compile-time verification across module boundaries
- (-) No independent scaling of modules
- (-) Single point of failure (mitigated by K8s restart policy)
- (-) Full rebuild for any module change

**Boundary**: Split into workspace if `cargo check` exceeds 30 seconds.

---

## ADR-002: Rust with `forbid(unsafe_code)`

**Status**: Accepted

**Context**: The platform handles auth, secrets, and multi-tenant isolation. Memory safety bugs could lead to privilege escalation or data leaks.

**Decision**: Use Rust with `unsafe_code = "forbid"` in `Cargo.toml` lints.

**Consequences**:
- (+) Eliminates entire classes of vulnerabilities (buffer overflows, use-after-free, data races)
- (+) Zero-cost abstractions for performance-critical paths
- (+) Strong type system catches logic errors at compile time
- (-) Steeper learning curve
- (-) Longer compile times (~30s for clean build)

---

## ADR-003: rustls, Ban openssl

**Status**: Accepted

**Context**: OpenSSL has a long history of security vulnerabilities and is a C dependency that complicates builds and supply chain auditing.

**Decision**: Use rustls for all TLS. `deny.toml` explicitly bans `openssl` and `openssl-sys` crates.

**Consequences**:
- (+) Pure-Rust TLS stack, no C dependencies
- (+) Auditable dependency chain
- (+) Consistent behavior across platforms
- (-) Some ecosystem crates require feature flags to use rustls instead of openssl

---

## ADR-004: sqlx Compile-Time Queries over ORM

**Status**: Accepted

**Context**: Need database access with strong type safety. ORMs (Diesel, SeaORM) abstract SQL but can generate suboptimal queries and hide database behavior.

**Decision**: Use sqlx with `query!` and `query_as!` macros for compile-time checked SQL. No ORM layer.

**Consequences**:
- (+) SQL queries validated against real schema at compile time
- (+) Full control over query optimization
- (+) No N+1 query surprises
- (-) Requires `just db-prepare` after query changes (generates `.sqlx/` offline cache)
- (-) Migrations must be kept in sync with code

---

## ADR-005: Valkey (Redis Fork) over Redis

**Status**: Accepted

**Context**: Need an in-memory cache for permission resolution, rate limiting, pub/sub, and session state. Redis changed to a non-open-source license.

**Decision**: Use Valkey (the Linux Foundation fork of Redis) via the `fred` async client.

**Consequences**:
- (+) Fully open-source (BSD license)
- (+) API-compatible with Redis
- (+) `fred` client is async-native Rust with connection pooling
- (-) Smaller community than Redis (growing)

---

## ADR-006: MinIO (S3 API) for Object Storage

**Status**: Accepted

**Context**: Need object storage for build artifacts, Parquet files, Git LFS objects, and OCI registry blobs.

**Decision**: Use MinIO with the `opendal` abstraction layer (S3 API).

**Consequences**:
- (+) S3-compatible — can migrate to AWS S3, GCS, or Azure Blob in the future
- (+) `opendal` abstracts the storage provider
- (+) Self-hosted, no cloud dependency for development
- (-) Single-node MinIO in dev; production needs cluster mode or external S3

---

## ADR-007: Embedded SPA (rust-embed) over Separate Frontend

**Status**: Accepted

**Context**: The web UI could be served by a separate web server (nginx, CDN) or embedded in the binary.

**Decision**: Embed the Preact SPA in the Rust binary using `rust-embed`.

**Consequences**:
- (+) Single artifact — no separate frontend deployment
- (+) No CORS configuration needed (same origin)
- (+) Atomic deploys — UI and API always in sync
- (-) UI changes require full binary rebuild
- (-) No CDN caching for static assets

---

## ADR-008: Preact over React for UI

**Status**: Accepted

**Context**: Need a component-based UI framework. React is the most popular but has a 40KB+ runtime.

**Decision**: Use Preact (3KB runtime) with esbuild for bundling.

**Consequences**:
- (+) 10x smaller bundle size
- (+) esbuild is 100x faster than webpack
- (+) API-compatible with React (via preact/compat)
- (-) Smaller ecosystem (most React libraries still work)
- (-) Less tooling and IDE support

---

## ADR-009: Per-Project K8s Namespaces for Isolation

**Status**: Accepted

**Context**: Multiple tenants (projects) run workloads on the same cluster. Need isolation for security and resource management.

**Decision**: Each project gets dedicated namespaces: `{slug}-dev`, `{slug}-staging`, `{slug}-production`. Pipeline and agent pods run in the project's namespace.

**Consequences**:
- (+) Namespace-level isolation (NetworkPolicy, ResourceQuota)
- (+) Clear resource ownership
- (+) Independent cleanup per project
- (-) Namespace proliferation (preview environments add more)
- (-) Requires namespace lifecycle management and cleanup

---

## ADR-010: Agents as First-Class Users with Delegation

**Status**: Accepted

**Context**: AI agents (Claude Code) need to perform actions on behalf of users. Could use separate auth system or integrate with existing RBAC.

**Decision**: Agents are users with `user_type = 'agent'`. They get ephemeral identities with delegated, time-bounded permissions from the requesting human.

**Consequences**:
- (+) Uniform RBAC — same permission model for humans and agents
- (+) Audit trail captures agent actions with delegation chain
- (+) Permissions are scoped and time-bounded
- (-) Agent identity lifecycle tied to session lifecycle
- (-) More complex identity management (ephemeral users, cleanup)

---

## ADR-011: Event-Driven + Reconciliation (Not Pure Event Sourcing)

**Status**: Accepted

**Context**: Pipeline execution and deployment need to react to events (git push, MR merge) but also recover from crashes.

**Decision**: Hybrid approach — `Arc<Notify>` for immediate event wake-up, polling loops (5-10s) for crash recovery. State is stored in Postgres, not an event log.

**Consequences**:
- (+) Immediate response to events (no polling delay)
- (+) Crash recovery via reconciliation (re-reads desired state from DB)
- (+) Simple mental model — state is in the database
- (-) Slight delay on recovery (up to 10s poll interval)
- (-) Not a full event sourcing system (no event replay)

---

## ADR-012: OTLP + Parquet over External Observability Stack

**Status**: Accepted

**Context**: Need observability (traces, logs, metrics) for tenant workloads. Could use external tools (Jaeger, Grafana, Prometheus) or build in.

**Decision**: Built-in OTLP ingest with Parquet cold storage on MinIO. Two-tier: Postgres for hot data (48h), Parquet/MinIO for cold (90d+).

**Consequences**:
- (+) No external observability dependencies
- (+) Unified query API through the platform
- (+) Parquet enables efficient columnar queries for cold data
- (-) Limited query capabilities compared to Grafana/ClickHouse
- (-) No built-in dashboarding (query API + web UI only)

---

## ADR-013: Built-in OCI Registry over External

**Status**: Accepted

**Context**: Pipeline-built container images need a registry. Could use external (Harbor, Docker Hub) or build in.

**Decision**: Built-in OCI v2 registry with Postgres metadata and MinIO blob storage.

**Consequences**:
- (+) No external registry dependency
- (+) Integrated auth (same RBAC model)
- (+) Garbage collection aligned with project lifecycle
- (-) Limited features compared to Harbor (no image signing, vulnerability scanning)
- (-) Single-node only

---

## ADR-014: Kind for Dev Clusters (Not OrbStack/Minikube)

**Status**: Accepted

**Context**: Development and testing need a local K8s cluster. Options: Kind, Minikube, OrbStack, k3s.

**Decision**: Use Kind (Kubernetes IN Docker) with port-mapped services.

**Consequences**:
- (+) Lightweight, fast startup
- (+) Reproducible via `just cluster-up`
- (+) Docker-based — works on all platforms
- (-) Single-node only (no multi-node testing)
- (-) Port conflicts if other containers use 5432/6379/8080/9000
