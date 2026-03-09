/**
 * Account settings Playwright tests.
 *
 * Prerequisites: a running Platform binary with a test database.
 *   cargo run                      # or: just run
 *   PLATFORM_URL=http://localhost:8080 npx playwright test account-flows
 *
 * Uses Chromium CDP virtual authenticator for WebAuthn flows.
 */
import { test, expect, type Page, type CDPSession } from '@playwright/test';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const ADMIN_USER = 'admin';
const ADMIN_PASS = process.env.PLATFORM_ADMIN_PASSWORD || 'admin';
const BASE_URL = process.env.PLATFORM_URL || 'http://localhost:8080';

async function dismissOnboarding(page: Page) {
  await page.evaluate(() => {
    document.querySelector('.onboarding-overlay')?.setAttribute('style', 'display:none');
    document.querySelector('.onboarding-backdrop')?.setAttribute('style', 'display:none');
  });
}

async function apiLogin(page: Page, user = ADMIN_USER, pass = ADMIN_PASS) {
  const resp = await page.request.post(`${BASE_URL}/api/auth/login`, {
    data: { name: user, password: pass },
  });
  expect(resp.ok(), `apiLogin: ${resp.status()} ${await resp.text()}`).toBeTruthy();
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({ timeout: 10_000 });
  await dismissOnboarding(page);
}

async function login(page: Page, user: string, pass: string) {
  await page.goto('/');
  await expect(page.locator('.login-card')).toBeVisible({ timeout: 10_000 });
  await page.fill('input[type="text"]', user);
  await page.fill('input[type="password"]', pass);
  await page.click('button[type="submit"]');
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({ timeout: 10_000 });
  await dismissOnboarding(page);
}

async function logout(page: Page) {
  // Click user menu, then logout
  await page.locator('.user-menu .btn').click();
  await page.locator('.user-menu-logout').click();
  await expect(page.locator('.login-card')).toBeVisible({ timeout: 10_000 });
}

async function apiCreateUser(page: Page, name: string, email: string, password: string): Promise<string> {
  const resp = await page.request.post(`${BASE_URL}/api/users`, {
    data: { name, email, password },
  });
  expect(resp.ok(), `createUser: ${resp.status()}`).toBeTruthy();
  const body = await resp.json();
  return body.id;
}

async function setupVirtualAuthenticator(page: Page): Promise<{ cdpSession: CDPSession; authenticatorId: string }> {
  const cdpSession = await page.context().newCDPSession(page);
  await cdpSession.send('WebAuthn.enable');
  const { authenticatorId } = await cdpSession.send('WebAuthn.addVirtualAuthenticator', {
    options: {
      protocol: 'ctap2',
      transport: 'internal',
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
    },
  });
  return { cdpSession, authenticatorId };
}

// ---------------------------------------------------------------------------
// Flow 1: Password change via UI
// ---------------------------------------------------------------------------

test.describe('Account: Password change', () => {
  test('change password, logout, verify new password works', async ({ page }) => {
    await apiLogin(page);

    const name = `pw-ui-${Date.now()}`;
    const email = `${name}@test.com`;
    const oldPass = 'oldpass123';
    const newPass = 'newpass456';

    await apiCreateUser(page, name, email, oldPass);

    // Logout admin, login as test user
    await logout(page);
    await login(page, name, oldPass);

    // Navigate to account settings
    await page.goto('/settings/account');
    await dismissOnboarding(page);
    await expect(page.getByRole('heading', { name: 'Account Settings' })).toBeVisible({ timeout: 5_000 });

    // Fill password change form
    const form = page.locator('.card').first();
    await form.locator('input[type="password"]').nth(0).fill(oldPass);
    await form.locator('input[type="password"]').nth(1).fill(newPass);
    await form.locator('input[type="password"]').nth(2).fill(newPass);
    await form.locator('button[type="submit"]').click();

    // Expect success message
    await expect(form.locator('.success-msg')).toBeVisible({ timeout: 5_000 });

    // Logout
    await logout(page);

    // Login with old password should fail
    await page.fill('input[type="text"]', name);
    await page.fill('input[type="password"]', oldPass);
    await page.click('button[type="submit"]');
    await expect(page.locator('.error-msg')).toBeVisible({ timeout: 5_000 });

    // Login with new password should succeed
    await page.fill('input[type="password"]', newPass);
    await page.click('button[type="submit"]');
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({ timeout: 10_000 });
  });
});

// ---------------------------------------------------------------------------
// Flow 2: Register passkey via UI
// ---------------------------------------------------------------------------

test.describe('Account: Passkey management', () => {
  test('register passkey appears in table', async ({ page }) => {
    await apiLogin(page);
    await page.goto('/settings/account');
    await dismissOnboarding(page);

    const { cdpSession } = await setupVirtualAuthenticator(page);

    // Click "Register New Passkey"
    await page.click('button:has-text("Register New Passkey")');
    await expect(page.locator('.modal')).toBeVisible();

    // Fill name and register
    await page.locator('.modal input[type="text"]').fill('My Test Key');
    await page.locator('.modal button:has-text("Register")').click();

    // Modal closes, passkey appears in table
    await expect(page.locator('.modal')).not.toBeVisible({ timeout: 10_000 });
    await expect(page.locator('table td:has-text("My Test Key")')).toBeVisible({ timeout: 5_000 });

    await cdpSession.detach();
  });

  // Flow 3: Rename passkey
  test('rename passkey updates table', async ({ page }) => {
    await apiLogin(page);
    await page.goto('/settings/account');
    await dismissOnboarding(page);

    const { cdpSession } = await setupVirtualAuthenticator(page);

    // Register a passkey first
    await page.click('button:has-text("Register New Passkey")');
    await page.locator('.modal input[type="text"]').fill('Old Name');
    await page.locator('.modal button:has-text("Register")').click();
    await expect(page.locator('.modal')).not.toBeVisible({ timeout: 10_000 });
    await expect(page.locator('table td:has-text("Old Name")')).toBeVisible({ timeout: 5_000 });

    // Click rename
    await page.click('button:has-text("Rename")');
    await expect(page.locator('.modal')).toBeVisible();
    await page.locator('.modal input[type="text"]').fill('New Name');
    await page.locator('.modal button:has-text("Save")').click();

    // Verify updated
    await expect(page.locator('.modal')).not.toBeVisible({ timeout: 5_000 });
    await expect(page.locator('table td:has-text("New Name")')).toBeVisible({ timeout: 5_000 });

    await cdpSession.detach();
  });

  // Flow 4: Delete passkey
  test('delete passkey removes from table', async ({ page }) => {
    await apiLogin(page);
    await page.goto('/settings/account');
    await dismissOnboarding(page);

    const { cdpSession } = await setupVirtualAuthenticator(page);

    // Register a passkey
    await page.click('button:has-text("Register New Passkey")');
    await page.locator('.modal input[type="text"]').fill('To Delete');
    await page.locator('.modal button:has-text("Register")').click();
    await expect(page.locator('.modal')).not.toBeVisible({ timeout: 10_000 });
    await expect(page.locator('table td:has-text("To Delete")')).toBeVisible({ timeout: 5_000 });

    // Accept the confirm dialog
    page.on('dialog', dialog => dialog.accept());

    // Click delete
    await page.click('button:has-text("Delete")');

    // Passkey should be gone
    await expect(page.locator('table td:has-text("To Delete")')).not.toBeVisible({ timeout: 5_000 });

    await cdpSession.detach();
  });
});

// ---------------------------------------------------------------------------
// Flow 5: Passkey login via UI
// ---------------------------------------------------------------------------

test.describe('Account: Passkey login', () => {
  test('register passkey then login with it', async ({ page }) => {
    await apiLogin(page);

    const name = `pk-login-${Date.now()}`;
    const email = `${name}@test.com`;
    const password = 'testpass123';

    await apiCreateUser(page, name, email, password);

    // Logout admin, login as test user
    await logout(page);
    await login(page, name, password);

    // Navigate to account settings, setup virtual authenticator, register passkey
    await page.goto('/settings/account');
    await dismissOnboarding(page);

    const { cdpSession } = await setupVirtualAuthenticator(page);

    await page.click('button:has-text("Register New Passkey")');
    await page.locator('.modal input[type="text"]').fill('Login Key');
    await page.locator('.modal button:has-text("Register")').click();
    await expect(page.locator('.modal')).not.toBeVisible({ timeout: 10_000 });
    await expect(page.locator('table td:has-text("Login Key")')).toBeVisible({ timeout: 5_000 });

    // Logout
    await logout(page);

    // Click "Sign in with Passkey"
    await expect(page.locator('button:has-text("Sign in with Passkey")')).toBeVisible();
    await page.click('button:has-text("Sign in with Passkey")');

    // Should redirect to dashboard
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({ timeout: 10_000 });

    await cdpSession.detach();
  });
});
