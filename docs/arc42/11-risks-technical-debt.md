# 11. Risks and Technical Debt

## Risks

| ID | Risk | Probability | Impact | Mitigation |
|---|---|---|---|---|
| R-1 | **Single binary = single point of failure** | Medium | High | K8s restart policy; reconciliation loops recover state from DB |
| R-2 | **No HA mode (single replica)** | High | High | Background tasks use optimistic locking; future: leader election for multi-replica |
| R-3 | **Kind dev cluster ≠ production topology** | High | Medium | No production validation yet; aspirational deployment view exists |
| R-4 | **Namespace proliferation** | Medium | Low | TTL-based cleanup for preview environments; periodic `just test-cleanup` |
| R-5 | **Vertical scaling limit** | Low | Medium | Single Postgres, single Valkey, single MinIO; monitor and scale infra before platform |
| R-6 | **Claude API dependency** | Medium | Medium | Agent sessions fail if Anthropic API is unavailable; no fallback LLM provider yet |

## Technical Debt

| ID | Type | Item | Impact | Module |
|---|---|---|---|---|
| D-1 | Missing feature | Force-push rejection not implemented (Plan 03 gap) | Low | `git/` |
| D-2 | Test gap | Some missing unit tests in secrets/pipeline definition | Medium | `secrets/`, `pipeline/` |
| D-3 | Design gap | Secret request flow: SSE not published, missing ProgressKind variant, scope hardcoded, pipeline injection missing | Medium | `secrets/` |
| D-4 | Test tier mismatch | Some E2E tests are actually single-endpoint tests (pending migration to integration tier) | Low | `tests/` |
| D-5 | No multi-replica support | Background tasks assume single instance; no leader election | High | `main.rs` |
| D-6 | No backup strategy | PostgreSQL and MinIO data not backed up in dev; production needs CNPG backup | Medium | Infrastructure |
| D-7 | Limited observability queries | Built-in query API is basic compared to Grafana/ClickHouse | Low | `observe/` |
| D-8 | No image vulnerability scanning | Built-in OCI registry lacks security scanning (Harbor has this) | Medium | `registry/` |

## Monitoring Recommendations

| Metric | Threshold | Action |
|---|---|---|
| Pipeline executor queue depth | > 20 pending | Investigate slow pods or K8s scheduling issues |
| Reconciler lag (time since last successful reconcile) | > 60s | Check deployer logs for errors |
| Permission cache hit rate | < 80% | Increase TTL or investigate cache invalidation frequency |
| Postgres connection pool exhaustion | > 90% used | Increase pool size or optimize long-running queries |
| Agent session reaper backlog | > 10 stale sessions | Check pod deletion failures |
