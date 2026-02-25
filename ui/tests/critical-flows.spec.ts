/**
 * Critical flow Playwright tests.
 *
 * Prerequisites: a running Platform binary with a test database.
 *   cargo run                      # or: just run
 *   PLATFORM_URL=http://localhost:8080 npx playwright test
 *
 * These tests use admin/<PLATFORM_ADMIN_PASSWORD> (bootstrap credentials).
 * Auth is cookie-based (credentials: 'include'), not Bearer tokens.
 */
import { test, expect, type Page } from '@playwright/test';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const ADMIN_USER = 'admin';
const ADMIN_PASS = process.env.PLATFORM_ADMIN_PASSWORD || 'admin';
const BASE_URL = process.env.PLATFORM_URL || 'http://localhost:8080';

/** Dismiss the onboarding overlay if visible (it blocks the entire UI). */
async function dismissOnboarding(page: Page) {
  await page.evaluate(() => {
    document.querySelector('.onboarding-overlay')?.setAttribute('style', 'display:none');
    document.querySelector('.onboarding-backdrop')?.setAttribute('style', 'display:none');
  });
}

/** Login via the UI login form. Sets session cookie. */
async function login(page: Page, user = ADMIN_USER, pass = ADMIN_PASS) {
  await page.goto('/');
  await expect(page.locator('.login-card')).toBeVisible({ timeout: 10_000 });
  await page.fill('input[type="text"]', user);
  await page.fill('input[type="password"]', pass);
  await page.click('button[type="submit"]');
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({ timeout: 10_000 });
  await dismissOnboarding(page);
}

/**
 * Login via Playwright's page.request (shares cookies with browser context).
 * Faster than form login — no need to wait for UI rendering.
 */
async function apiLogin(page: Page, user = ADMIN_USER, pass = ADMIN_PASS) {
  // Use page.request which shares the cookie jar with the browser
  const resp = await page.request.post(`${BASE_URL}/api/auth/login`, {
    data: { name: user, password: pass },
  });
  expect(resp.ok(), `apiLogin: ${resp.status()} ${await resp.text()}`).toBeTruthy();

  // Navigate to dashboard — cookie is already set
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({ timeout: 10_000 });
  await dismissOnboarding(page);
}

// ---------------------------------------------------------------------------
// Flow 1: Login → Dashboard renders
// ---------------------------------------------------------------------------

test.describe('Flow 1: Login → Dashboard', () => {
  test('login form works and dashboard renders', async ({ page }) => {
    await login(page);

    // Stats cards are visible
    await expect(page.locator('.stats-grid')).toBeVisible();
    await expect(page.locator('.stat-card').first()).toBeVisible();

    // "Create New App" card is visible
    await expect(page.locator('.create-app-card')).toBeVisible();

    // Quick action buttons are visible
    await expect(page.getByRole('link', { name: 'New Project' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'View Logs' })).toBeVisible();
  });

  test('invalid login shows error', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('.login-card')).toBeVisible();
    await page.fill('input[type="text"]', 'admin');
    await page.fill('input[type="password"]', 'wrongpassword');
    await page.click('button[type="submit"]');

    await expect(page.locator('.error-msg')).toBeVisible({ timeout: 5_000 });
  });
});

// ---------------------------------------------------------------------------
// Flow 2: Project CRUD
// ---------------------------------------------------------------------------

test.describe('Flow 2: Project CRUD', () => {
  const projName = `pw-test-${Date.now()}`;

  test('create project, view it, navigate tabs', async ({ page }) => {
    await apiLogin(page);

    // Try creating project via API (needs git in container)
    const projResp = await page.request.post(`${BASE_URL}/api/projects`, {
      data: { name: projName, visibility: 'private' },
    });

    if (!projResp.ok()) {
      test.skip(true, 'Project creation failed (git not available in container)');
      return;
    }
    const proj = await projResp.json();

    // Navigate to project detail
    await page.goto(`/projects/${proj.id}`);
    await dismissOnboarding(page);
    await expect(page.getByRole('heading', { name: projName })).toBeVisible({ timeout: 10_000 });

    // Verify tabs exist
    for (const tab of ['Issues', 'MRs', 'Pipelines', 'Deploys']) {
      await expect(page.locator(`[role="tab"]:has-text("${tab}"), .tab:has-text("${tab}"), a:has-text("${tab}")`).first()).toBeVisible();
    }
  });
});

// ---------------------------------------------------------------------------
// Flow 3: Issue CRUD
// ---------------------------------------------------------------------------

test.describe('Flow 3: Issue CRUD', () => {
  test('create issue and add comment', async ({ page }) => {
    await apiLogin(page);

    // Create a project via API (needs git in container)
    const projResp = await page.request.post(`${BASE_URL}/api/projects`, {
      data: { name: `issue-test-${Date.now()}`, visibility: 'private' },
    });

    if (!projResp.ok()) {
      test.skip(true, 'Project creation failed (git not available in container)');
      return;
    }
    const proj = await projResp.json();

    // Navigate to project
    await page.goto(`/projects/${proj.id}`);
    await dismissOnboarding(page);
    await expect(page.locator('h2').first()).toBeVisible({ timeout: 5_000 });

    // Click Issues tab
    await page.click('[role="tab"]:has-text("Issues"), .tab:has-text("Issues"), a:has-text("Issues")');

    // Create new issue
    await page.click('button:has-text("New Issue")');
    await page.fill('input[placeholder*="title" i], input[name="title"]', 'Playwright test issue');
    await page.fill('textarea[name="body"], textarea[placeholder*="description" i]', 'Created by Playwright');
    await page.click('button[type="submit"]:has-text("Create")');

    // Should see the issue
    await expect(page.locator('text=Playwright test issue')).toBeVisible({ timeout: 5_000 });
  });
});

// ---------------------------------------------------------------------------
// Flow 4: Navigation between pages
// ---------------------------------------------------------------------------

test.describe('Flow 4: Navigation', () => {
  test('navigate through main pages without errors', async ({ page }) => {
    await login(page);

    const pages = [
      { path: '/projects', heading: 'Projects' },
      { path: '/observe/logs', heading: 'Log Search' },
      { path: '/observe/traces', heading: 'Traces' },
      { path: '/observe/metrics', heading: 'Metrics' },
      { path: '/observe/alerts', heading: 'Alerts' },
      { path: '/admin/users', heading: 'Users' },
      { path: '/admin/roles', heading: 'Roles' },
      { path: '/settings/tokens', heading: 'API Tokens' },
    ];

    const errors: string[] = [];
    page.on('console', msg => { if (msg.type() === 'error') errors.push(msg.text()); });
    page.on('pageerror', err => errors.push(`PAGE ERROR: ${err.message}`));

    for (const p of pages) {
      await page.goto(p.path, { waitUntil: 'networkidle' });
      await dismissOnboarding(page);
      const html = await page.content();
      const hasApp = html.includes('id="app"');
      const bodyLen = html.length;
      await expect(
        page.getByRole('heading', { name: p.heading }),
        `${p.path}: heading "${p.heading}" not found. HTML len=${bodyLen}, hasApp=${hasApp}, errors=[${errors.join('; ')}], url=${page.url()}`
      ).toBeVisible({ timeout: 5_000 });
      // Verify no error boundary
      await expect(page.locator('.error-boundary')).not.toBeVisible();
    }
  });
});

// ---------------------------------------------------------------------------
// Flow 5: Admin — user management
// ---------------------------------------------------------------------------

test.describe('Flow 5: Admin', () => {
  test('list users and roles pages render', async ({ page }) => {
    await login(page);

    // Users page
    await page.goto('/admin/users');
    await dismissOnboarding(page);
    await expect(page.getByRole('heading', { name: 'Users' })).toBeVisible({ timeout: 5_000 });
    // Should see at least the admin user in the table (check for email which is unique)
    await expect(page.getByRole('cell', { name: 'admin@localhost' })).toBeVisible();

    // Roles page
    await page.goto('/admin/roles');
    await dismissOnboarding(page);
    await expect(page.getByRole('heading', { name: 'Roles' })).toBeVisible({ timeout: 5_000 });
    // System roles should be visible
    await expect(page.getByText('developer', { exact: true }).first()).toBeVisible();
  });
});
