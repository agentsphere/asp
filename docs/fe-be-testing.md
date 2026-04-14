# Frontend–Backend Integration Testing

Three tiers prevent type drift between the Rust API and the Preact UI.

## Tier 1: ts-rs Auto-Generated Types

Rust response structs derive `#[derive(TS)]` which generates TypeScript type definitions at test time. This makes field name/type mismatches impossible by construction.

### How it works

1. Every response struct in `src/api/*.rs`, `src/observe/*.rs`, `src/git/browser.rs`, and `src/secrets/engine.rs` has `#[derive(TS)]` + `#[ts(export)]`
2. Running `just types` executes the `export_bindings` tests, which write `.ts` files to `ui/src/lib/generated/`
3. `ui/src/lib/types.ts` re-exports all generated types — UI imports don't change
4. `serde(rename)` attributes propagate automatically (e.g. `for_seconds` → `window_seconds`)

### Commands

```bash
just types          # generate types + typecheck
```

### Adding a new response struct

```rust
#[derive(Debug, Serialize, TS)]
#[ts(export, rename = "MyThing")]    // rename controls the TS type name
pub struct MyThingResponse {
    pub id: Uuid,
    pub name: String,
    #[ts(type = "number")]           // override i64 → bigint default
    pub count: i64,
    #[ts(type = "Record<string, any> | null")]  // override serde_json::Value
    pub metadata: Option<serde_json::Value>,
}
```

Then add to `ui/src/lib/types.ts`:
```typescript
export type { MyThing } from './generated/MyThing';
```

### Common annotations

| Rust type | Default TS | Override needed? |
|---|---|---|
| `Uuid` | `string` | No |
| `DateTime<Utc>` | `string` | No |
| `i32` | `number` | No |
| `i64` | `bigint` | Yes — add `#[ts(type = "number")]` |
| `Option<i64>` | `bigint \| null` | Yes — add `#[ts(type = "number \| null")]` |
| `serde_json::Value` | `JsonValue` (union) | Yes if UI needs specific shape |
| `Vec<String>` | `Array<string>` | No |

## Tier 2: Contract Integration Tests

`tests/ui_contract.rs` — 33 tests that hit real API endpoints and assert JSON shapes match what the UI expects.

### What they check

- Field names exist (catches serde rename bugs)
- Field types are correct (string, number, bool, null)
- List endpoints return `{items: [...], total: N}` wrapper
- Nullable fields can actually be null
- UUIDs parse as UUIDs, timestamps contain `T`

### Running

```bash
# Requires Postgres + Valkey running (just cluster-up)
cargo nextest run --test ui_contract

# Or via just (DATABASE_URL set automatically)
just test-contract
```

### Adding a contract test

Follow the existing pattern — one test per endpoint or logical group:

```rust
#[sqlx::test(migrations = "./migrations")]
async fn contract_my_endpoint(pool: PgPool) {
    let (state, admin_token) = test_state(pool).await;
    let app = test_router(state);

    let (status, body) = helpers::get_json(&app, &admin_token, "/api/my-endpoint").await;

    assert_eq!(status, StatusCode::OK);
    // For list endpoints:
    let items = assert_list_response(&body, "my-endpoint");
    // For single objects:
    assert_uuid(&body["id"], "thing.id");
    assert!(body["name"].is_string(), "missing name");
}
```

Helper functions: `assert_uuid`, `assert_timestamp`, `assert_number`, `assert_list_response`.

## Tier 3: Playwright Browser E2E

`ui/tests/critical-flows.spec.ts` — 5 flows that test the full stack through a real browser.

### Flows

1. **Login → Dashboard** — form submission, stats render, quick actions visible
2. **Project CRUD** — create project, verify detail page, check tabs
3. **Issue CRUD** — create issue via UI, verify it appears
4. **Navigation** — visit all major pages, verify no error boundaries
5. **Admin** — users and roles pages render with data

### Running

```bash
# 1. Start the server (separate terminal)
just run

# 2. Run Playwright tests
just ui test

# Or with custom URL:
PLATFORM_URL=http://localhost:8080 cd ui && npx playwright test
```

### Adding a flow test

```typescript
test('my new flow', async ({ page }) => {
  await apiLogin(page);  // fast login via API + localStorage
  await page.goto('/my-page');
  await expect(page.locator('h2')).toContainText('My Page');
  // ... interactions and assertions
});
```

Use `apiLogin()` for speed (skips the login form). Use `login()` only when testing the login flow itself.

## When to use which tier

| Scenario | Tier |
|---|---|
| Added/renamed a field on a response struct | Tier 1 (`just types`) |
| Changed endpoint path or HTTP method | Tier 2 (contract test) |
| Changed list endpoint to return `Vec<T>` instead of `ListResponse<T>` | Tier 2 |
| New page or form in the UI | Tier 3 (Playwright) |
| Refactored UI routing | Tier 3 |
| Serde rename attribute changed | Tier 1 catches it automatically |

## CI integration

```
just ci              # includes test-unit (has ts-rs export tests) + test-contract + test-api
just ui test         # run separately (needs running server)
just ci-full         # ci + test-k8s + test-e2e (excludes Playwright)
```
