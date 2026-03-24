# 2. Constraints

## Technical Constraints

| Constraint | Rationale |
|---|---|
| **Rust (edition 2024), `forbid(unsafe_code)`** | Memory safety without GC; unsafe forbidden in `Cargo.toml` lints |
| **rustls only — openssl banned** | `deny.toml` blocks `openssl`/`openssl-sys`; pure-Rust TLS stack |
| **Single crate** | No workspace split unless `cargo check` exceeds 30s; currently ~23K LOC in 11 modules |
| **sqlx compile-time checking** | All queries use `sqlx::query!` / `sqlx::query_as!`; CI uses `SQLX_OFFLINE=true` |
| **Kubernetes-native** | Pipeline steps, agent sessions, and deployments all run as K8s pods |
| **PostgreSQL as primary store** | ACID transactions, 28 migration pairs, compile-time validated queries |
| **Valkey (Redis fork) for caching** | Permission cache, rate limiting, pub/sub, session state |
| **MinIO (S3 API) for objects** | Build artifacts, Parquet files, LFS objects, OCI blobs |

## Organizational Constraints

| Constraint | Impact |
|---|---|
| **Small team** | Single binary simplifies deployment and ops; no separate infra team |
| **Kind for development** | Local K8s cluster with port-mapped Postgres, Valkey, MinIO; not yet validated on production K8s |
| **AI-first audience** | UI is secondary to API and MCP server interfaces; agents are primary users |

## Conventions

| Convention | Enforced By |
|---|---|
| `CLAUDE.md` coding standards | AI agent context; human reference |
| `just` task runner | 30+ recipes for build, test, deploy, coverage |
| Pre-commit hooks | `rustfmt --check`, `clippy`, YAML validation |
| Commit `Cargo.lock` | Binary project convention |
| Commit `.sqlx/` cache | Enables offline CI builds |
| Reversible migrations | `just db-add` creates up/down pairs |
| 100% diff coverage | `just cov-diff-check` enforces on changed lines |
