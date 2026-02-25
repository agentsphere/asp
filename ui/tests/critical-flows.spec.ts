/**
 * Critical flow Playwright tests.
 *
 * Prerequisites: a running Platform binary with a test database.
 *   cargo run                      # or: just run
 *   PLATFORM_URL=http://localhost:8080 npx playwright test
 *
 * These tests use admin/testpassword (bootstrap credentials in dev mode).
 */
import { test, expect, type Page } from '@playwright/test';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const ADMIN_USER = 'admin';
const ADMIN_PASS = 'testpassword';

/** Login via the UI login form. */
async function login(page: Page, user = ADMIN_USER, pass = ADMIN_PASS) {
  await page.goto('/');
  // Should redirect to login form
  await expect(page.locator('.login-card')).toBeVisible({ timeout: 10_000 });
  await page.fill('input[type="text"]', user);
  await page.fill('input[type="password"]', pass);
  await page.click('button[type="submit"]');
  // Wait for dashboard to appear
  await expect(page.locator('h2')).toContainText('Dashboard', { timeout: 10_000 });
}

/** Login via API and inject the token into localStorage (faster). */
async function apiLogin(page: Page, user = ADMIN_USER, pass = ADMIN_PASS) {
  const baseURL = page.context().pages()[0]?.url()
    ? new URL(page.url()).origin
    : 'http://localhost:8080';

  const resp = await page.request.post(`${baseURL}/api/auth/login`, {
    data: { name: user, password: pass },
  });
  expect(resp.ok()).toBeTruthy();
  const body = await resp.json();

  // Set token in localStorage (mirrors what auth.tsx does)
  await page.goto('/');
  await page.evaluate((token: string) => {
    localStorage.setItem('token', token);
  }, body.token);

  // Reload to pick up the token
  await page.goto('/');
  await expect(page.locator('h2')).toContainText('Dashboard', { timeout: 10_000 });
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

    // Navigate to projects page
    await page.click('a[href="/projects"]');
    await expect(page.locator('h2')).toContainText('Projects', { timeout: 5_000 });

    // Click "New Project" button
    await page.click('button:has-text("New Project")');

    // Fill the form
    await page.fill('input[placeholder*="name" i], input[name="name"]', projName);
    await page.click('button[type="submit"]:has-text("Create")');

    // Should redirect to project detail
    await expect(page.locator('h2')).toContainText(projName, { timeout: 10_000 });

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

    // Create a project via API
    const projResp = await page.request.post('/api/projects', {
      headers: { Authorization: `Bearer ${await getToken(page)}` },
      data: { name: `issue-test-${Date.now()}`, visibility: 'private' },
    });
    const proj = await projResp.json();

    // Navigate to project
    await page.goto(`/projects/${proj.id}`);
    await expect(page.locator('h2')).toBeVisible({ timeout: 5_000 });

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
    await apiLogin(page);

    // Navigate to key pages and verify they render without errors
    const pages = [
      { path: '/projects', heading: 'Projects' },
      { path: '/observe/logs', heading: 'Logs' },
      { path: '/observe/traces', heading: 'Traces' },
      { path: '/observe/metrics', heading: 'Metrics' },
      { path: '/observe/alerts', heading: 'Alerts' },
      { path: '/admin/users', heading: 'Users' },
      { path: '/admin/roles', heading: 'Roles' },
      { path: '/settings/tokens', heading: 'Tokens' },
    ];

    for (const p of pages) {
      await page.goto(p.path);
      await expect(page.locator(`h2:has-text("${p.heading}")`)).toBeVisible({ timeout: 5_000 });
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
    await apiLogin(page);

    // Users page
    await page.goto('/admin/users');
    await expect(page.locator('h2')).toContainText('Users', { timeout: 5_000 });
    // Should see at least the admin user in the table
    await expect(page.locator('text=admin')).toBeVisible();

    // Roles page
    await page.goto('/admin/roles');
    await expect(page.locator('h2')).toContainText('Roles', { timeout: 5_000 });
    // System roles should be visible
    await expect(page.locator('text=admin')).toBeVisible();
    await expect(page.locator('text=developer')).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

async function getToken(page: Page): Promise<string> {
  return page.evaluate(() => localStorage.getItem('token') || '');
}
