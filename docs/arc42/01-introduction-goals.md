# 1. Introduction and Goals

## Business Context

Modern software teams need a broad set of tools to practice strong CI/CD and DevOps: version control, build pipelines, deployment orchestration, observability, secrets management, access control, and notifications. Typically this means operating and integrating 8+ separate systems — each with its own auth model, upgrade cycle, and failure modes. The operational overhead compounds as teams scale, and the integration surface becomes a source of fragility.

With the rise of AI-powered development agents, a new dimension emerges: these agents need **sandboxed execution environments** with network isolation (Kubernetes NetworkPolicies), ephemeral identities, and delegated permissions — capabilities that traditional tool stacks were never designed for.

The Platform addresses this by consolidating the full DevOps lifecycle into a single Rust binary:

| Capability | Platform Module | What It Does |
|---|---|---|
| Version control | `git/` | Git smart HTTP/SSH, LFS, file browser, branch protection |
| Build & test | `pipeline/` | CI/CD pipeline engine with K8s pod execution |
| Deployment | `deployer/` | GitOps reconciliation, canary deployments, preview environments |
| AI dev agents | `agent/` | Sandboxed agent sessions in isolated K8s namespaces with NetworkPolicies |
| Observability | `observe/` | OTLP ingest, Parquet cold storage, log/trace/metric queries, alerting |
| Auth & access | `auth/` + `rbac/` | Sessions, API tokens, passkeys, RBAC with delegation |
| Secrets | `secrets/` | AES-256-GCM encrypted secrets in Postgres |
| Notifications | `notify/` | Email (SMTP), webhooks (HMAC-SHA256), in-app notifications |
| Container images | `registry/` | Built-in OCI registry with garbage collection |

**Primary users**: AI agents (Claude Code) operating autonomously within sandboxed environments. Humans serve as auditors, reviewers, and operators.

**Kept as infrastructure**: PostgreSQL (CNPG), Valkey, MinIO, Kubernetes.

## Top 5 Functional Requirements

| # | Requirement | Module |
|---|---|---|
| F1 | Git hosting with smart HTTP/SSH, LFS, branch protection | `git/` |
| F2 | CI/CD pipelines triggered by push/MR/tag/API with K8s pod execution | `pipeline/` |
| F3 | Deployment orchestration with GitOps, canary progression, preview envs | `deployer/` |
| F4 | AI agent sessions in network-isolated sandboxes with ephemeral identities | `agent/` |
| F5 | Built-in observability: OTLP ingest, Parquet cold storage, alerting | `observe/` |

## Quality Goals

Ranked by priority:

| # | Quality Goal | Motivation |
|---|---|---|
| Q1 | **Security** | Multi-tenant isolation, NetworkPolicy sandboxes for agents, RBAC with delegation, encryption at rest, SSRF protection |
| Q2 | **Operability** | Single binary, minimal infra dependencies, self-healing reconciliation loops |
| Q3 | **Reliability** | State machine enforcement, crash-recovery via reconciliation, compile-time SQL |
| Q4 | **Efficiency** | Single process with shared state, no IPC overhead, connection pooling |
| Q5 | **Testability** | 4-tier testing pyramid (1,339 tests), compile-time query checking, 100% diff coverage |

## Stakeholders

| Role | Concern | How the Platform Addresses It |
|---|---|---|
| **Platform Operator** | Deployment simplicity, monitoring, upgrades | Single binary, health endpoints, env-var config, self-hosted by design |
| **Developer (tenant)** | Push code, review MRs, monitor pipelines/deploys | Git hosting, project management, web UI |
| **AI Agent (Claude Code)** | Autonomous code changes, branch management, deployments | Network-isolated sandboxes, ephemeral identity, delegated RBAC, MCP servers |
| **Security Auditor** | Access control, audit trail, secrets management | RBAC, audit_log table, AES-256-GCM encryption, rate limiting |
