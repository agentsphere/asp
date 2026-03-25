# 4. Solution Strategy

## Fundamental Decisions

| # | Decision | Motivation | Trade-off |
|---|---|---|---|
| 1 | **Single Rust binary** over microservices | Eliminates IPC overhead, simplifies deployment to one K8s Deployment + one IngressRoute, single container image | No independent scaling; single point of failure (mitigated by K8s restart policy) |
| 2 | **PostgreSQL as the brain** | Unified schema (64 migrations), compile-time validated queries via sqlx, ACID transactions for all state | Vertical scaling limit; no read replicas yet |
| 3 | **Event-driven + reconciliation** | `Notify` for immediate wake-up, polling loops for crash recovery; best of both worlds | Slight delay on recovery (up to 10s poll interval) |
| 4 | **Per-project K8s namespaces** | Tenant isolation via namespace boundaries, NetworkPolicy enforcement, resource quotas | Namespace proliferation; requires cleanup for preview environments |
| 5 | **Agents as first-class users** | Uniform RBAC for humans and AI; delegation model grants scoped, time-bounded permissions | Agent identity lifecycle tied to session lifecycle |
| 6 | **Embedded UI via rust-embed** | Single artifact, no CORS configuration needed, atomic deploys | UI changes require full binary rebuild |

## Technology Choices

| Area | Choice | Why Not the Alternative |
|---|---|---|
| Language | Rust | Memory safety without GC; `forbid(unsafe_code)` eliminates entire vulnerability class |
| TLS | rustls | Pure Rust; `deny.toml` bans openssl to avoid C dependency chain |
| HTTP | Axum 0.8 + Tower | Composable middleware, type-safe extractors, async-native |
| Database | sqlx (not an ORM) | Compile-time query validation; direct SQL control |
| Cache | Valkey via fred | Redis-compatible fork with active maintenance; fred client is async-native |
| Objects | MinIO via opendal | S3-compatible; opendal abstracts provider for future migration |
| K8s | kube-rs | Native Rust client; no kubectl shelling |
| UI | Preact + esbuild | 3KB runtime vs React's 40KB; esbuild is 100x faster than webpack |
| Auth | argon2 (passwords) + AES-256-GCM (secrets) + WebAuthn (passkeys) | Industry standard primitives; no custom crypto |

## Architecture Style

The platform follows a **modular monolith** pattern:

- 15 modules under `src/` with clear boundaries
- Modules communicate only through `AppState` — never import each other's internals
- Cross-module types live in `src/error.rs` or `src/config.rs`
- Each module owns its DB tables and defines its own error types
- Background tasks are spawned per-module and coordinate via `Arc<Notify>` signals

This is explicitly **not** microservices — there is no network boundary between modules. The single-crate constraint holds until `cargo check` exceeds 30 seconds.

## Quality Strategy

| Quality Goal | Strategy |
|---|---|
| Security | Input validation at boundary, RBAC on every handler, audit log on every mutation, rate limiting on auth endpoints |
| Operability | 87 env-var config knobs (12-factor), health endpoint reports all subsystems, graceful shutdown |
| Reliability | State machines with `can_transition_to()`, reconciliation loops for eventual consistency, compile-time SQL |
| Efficiency | Connection pooling (Postgres, Valkey), permission cache (95% DB load reduction), single-process shared memory |
| Testability | 4-tier pyramid (unit → integration → E2E → LLM), per-test DB isolation via `#[sqlx::test]`, mock CLI for agent tests |
