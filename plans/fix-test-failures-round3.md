# Plan: Fix 16 Remaining Test Failures (Round 3)

## Summary

16 failures in 3 categories:
- **12 audit log tests** — race condition: `tokio::spawn` audit write not complete before test queries DB
- **3 webhook tests** — same race: `tokio::spawn` dispatch not complete before `mock_server.verify()`
- **1 port conflict** — `start_test_server` port still in use despite `ServerGuard`

All share the same root cause: **`tokio::spawn` fire-and-forget tasks not completing before test assertions**.

---

## Category A: Audit log race condition (12 tests)

**Tests**: `test_store_credentials_audit_logged`, `test_delete_credentials_audit_logged`, `command_audit_logged`, `contract_audit_log_list`, `test_add_gpg_key_creates_audit_log`, `test_delete_gpg_key_creates_audit_log`, `delete_passkey_creates_audit_log`, `login_creates_audit_entry`, `role_assignment_creates_audit`, `delegation_audit_logged`, `test_add_ssh_key_creates_audit_log`, `test_delete_ssh_key_creates_audit_log`

**Root cause**: `src/audit.rs` `send_audit()` uses `tokio::spawn` to write asynchronously. The spawned task may not be polled/completed before the test's next line queries the `audit_log` table. `tokio::spawn` provides no ordering guarantee — it merely submits work to the runtime's task queue.

**Fix**: Add a `tokio::task::yield_now().await` (or `tokio::time::sleep(Duration::from_millis(50)).await`) in each audit test after the action that triggers the audit write, but before the DB query. This gives the spawned audit task a chance to execute.

Better approach: make `send_audit` return a `JoinHandle<()>` and add a `flush_audit()` test helper that awaits it. OR: change `AuditLog` to have an optional `flush()` method that stores handles and awaits them.

**Simplest correct fix**: Add a small helper to `src/audit.rs`:

```rust
/// Write an audit entry directly (blocking). Use in tests to avoid race conditions.
pub async fn write_audit(pool: &PgPool, entry: AuditEntry) {
    write_audit_inner(pool, &entry).await;
}
```

Then in the test callers that check audit, either:
1. Add `tokio::time::sleep(Duration::from_millis(100)).await` before querying, OR
2. Use a retry loop with short sleeps (poll until the row appears, timeout after 2s)

Option 2 is more robust. Add a helper to `tests/helpers/mod.rs`:

```rust
pub async fn wait_for_audit(pool: &PgPool, action: &str, max_ms: u64) -> i64 {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(max_ms);
    loop {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log WHERE action = $1"
        ).bind(action).fetch_one(pool).await.unwrap();
        if count > 0 { return count; }
        if tokio::time::Instant::now() > deadline { return 0; }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
```

Then update each failing test to use `wait_for_audit(&pool, "role.assign", 2000)` instead of the immediate COUNT query.

**Files**: `tests/helpers/mod.rs` (add helper), then 8 test files to use it:
- `tests/cli_auth_integration.rs`
- `tests/commands_integration.rs`
- `tests/contract_integration.rs`
- `tests/gpg_keys_integration.rs`
- `tests/passkey_integration.rs`
- `tests/rbac_integration.rs`
- `tests/ssh_keys_integration.rs`
- `tests/dashboard_integration.rs`

---

## Category B: Webhook dispatch race condition (3 tests)

**Tests**: `webhook_no_signature_without_secret`, `webhook_hmac_signature`, `webhook_fires_on_issue_create`

**Root cause**: `fire_webhooks()` uses `tokio::spawn` for each webhook delivery. The test calls `mock_server.verify()` immediately after creating the issue, but the spawned dispatch task hasn't executed yet. The wiremock server receives 0 requests.

**Fix**: Same pattern as audit — add a delay before `mock_server.verify()`. But wiremock's `verify()` doesn't have built-in polling. Use `tokio::time::sleep(Duration::from_millis(500)).await` before `mock_server.verify()`. This is simpler than audit because the verify already has a clear failure mode.

Alternatively, use wiremock's `Mock::expect(1).named("...").mount(&mock_server)` and `mock_server.received_requests()` in a poll loop.

**Simplest fix**: Add `tokio::time::sleep(Duration::from_millis(500)).await` before each `mock_server.verify()` call.

**Files**: `tests/webhook_integration.rs`

---

## Category C: Port conflict (1 test)

**Test**: `pull_platform_runner_image` (Address already in use)

**Root cause**: `start_test_server()` binds to `PLATFORM_LISTEN_PORT` (a fixed port). Even with `ServerGuard` aborting the task on drop, there's a timing issue — the TCP socket may linger in TIME_WAIT. The nextest `serial-listen-port` test group should serialize these, but the abort + rebind may still race.

**Fix**: Set `SO_REUSEADDR` on the listener socket before binding. In tokio, use `socket2` to create the socket with `SO_REUSEADDR`, then convert to `tokio::net::TcpListener`:

```rust
let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();
socket.set_reuse_address(true).unwrap();
socket.bind(&format!("0.0.0.0:{port}").parse::<std::net::SocketAddr>().unwrap().into()).unwrap();
socket.listen(128).unwrap();
socket.set_nonblocking(true).unwrap();
let listener = tokio::net::TcpListener::from_std(socket.into()).unwrap();
```

OR simpler: add a small sleep at the start of `start_test_server` to let the previous socket fully close. But `SO_REUSEADDR` is the proper fix.

Check if `socket2` is already a dependency. If not, the simplest fix is to add a 200ms sleep at the start of `start_test_server` as a workaround, since these tests are already serialized.

**Files**: `tests/helpers/mod.rs`

---

## Implementation Order

1. Add `wait_for_audit()` helper to `tests/helpers/mod.rs` → update 8 test files (fixes 12 tests)
2. Add sleep before `mock_server.verify()` in `tests/webhook_integration.rs` (fixes 3 tests)
3. Fix port reuse in `tests/helpers/mod.rs` `start_test_server()` (fixes 1 test)
