import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// This file starts its own Mimicway, on a port of its own, with AUTH_ENABLED=true: the other specs share the instance
// on :7342, which runs without authentication. It needs the binary built (`cargo build`) and frontend/dist, like the
// rest of the suite.
//
// With authentication on, only the management API asks for a token (auth_middleware, src/auth/middleware.rs): the
// files of the interface load without one, or a browser could not even reach the login screen.

const repoRoot = path.resolve(fileURLToPath(new URL('.', import.meta.url)), '..', '..');
const binaryPath = path.join(repoRoot, 'target', 'debug', process.platform === 'win32' ? 'mimicway.exe' : 'mimicway');
const staticDir = path.join(repoRoot, 'frontend', 'dist');

function getFreePort() {
  return new Promise((resolve, reject) => {
    const srv = createServer();
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
    srv.on('error', reject);
  });
}

async function waitForHealth(baseUrl, timeoutMs = 15000) {
  const start = Date.now();
  let lastError;
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(`${baseUrl}/api/health`);
      if (res.ok) return;
    } catch (e) {
      lastError = e;
    }
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error(
    `Mimicway (instance with authentication, started by this spec) did not start in time on ${baseUrl}: ${lastError}`,
  );
}

test.describe('Auth: the files of the interface load without a token (AUTH_ENABLED=true)', () => {
  let child;
  let baseUrl;
  let dataDir;

  test.beforeAll(async () => {
    const port = await getFreePort();
    baseUrl = `http://127.0.0.1:${port}`;
    dataDir = mkdtempSync(path.join(tmpdir(), 'mimicway-auth-e2e-'));

    child = spawn(binaryPath, [], {
      cwd: repoRoot,
      env: {
        ...process.env,
        PORT: String(port),
        STATIC_DIR: staticDir,
        DATA_PATH: dataDir,
        AUTH_ENABLED: 'true',
        // Never reached, since no login goes to Keycloak here: Mimicway refuses to start with authentication on and
        // these settings empty (AuthConfig::from_env).
        KEYCLOAK_URL: 'http://127.0.0.1:1',
        KEYCLOAK_REALM: 'test-realm',
        KEYCLOAK_CLIENT_ID: 'mimicway',
        SUPER_ADMINS: '',
      },
      stdio: 'pipe',
    });

    await waitForHealth(baseUrl);
  });

  test.afterAll(() => {
    if (child) {
      child.kill();
    }
    if (dataDir) {
      rmSync(dataDir, { recursive: true, force: true });
    }
  });

  test('the home page loads without a token and shows the login screen', async ({ page }) => {
    const response = await page.goto(baseUrl + '/');
    expect(response.status()).toBe(200);
    // The login form only shows once the bundle has run, not from the HTML alone: the interface asked
    // /api/auth/status (open without a token), found authentication on, and shows LoginForm.svelte.
    await expect(page.locator('[data-testid="login-form-username-input"]')).toBeVisible();
    await expect(page.locator('[data-testid="login-form-password-input"]')).toBeVisible();
    await docsScreenshot(
      page,
      'authentication-login-screen.png',
      '[data-testid="login-form-username-input"], [data-testid="login-form-submit-button"]',
      { reopenWaitingFor: '[data-testid="login-form-username-input"]' },
    );
  });

  test('a signed-in super-admin sees their name and the reset button', async ({ page }) => {
    // This instance reaches no Keycloak, so the login answer and the data are stubbed in the browser: the test checks
    // how the interface presents a signed-in user. Token validation itself is covered by the server's tests.
    await page.route('**/api/auth/login', (route) =>
      route.fulfill({
        json: { access_token: 'stub-token', refresh_token: null, username: 'alice', is_super_admin: true },
      }),
    );
    await page.route('**/api/services', (route) => route.fulfill({ json: [] }));
    await page.route('**/api/groups', (route) => route.fulfill({ json: [] }));
    // What a server built without the optional features answers to a valid token; the stub token would get 401,
    // which signs the user out.
    await page.route('**/api/messaging/status', (route) => route.fulfill({ status: 404, body: '' }));
    await page.route('**/api/tcp/status', (route) => route.fulfill({ status: 404, body: '' }));

    await page.goto(baseUrl + '/');
    await page.locator('[data-testid="login-form-username-input"]').fill('alice');
    await page.locator('[data-testid="login-form-password-input"]').fill('not-a-real-password');
    const loginAnswered = page.waitForResponse('**/api/auth/login');
    await page.locator('[data-testid="login-form-submit-button"]').click();
    await loginAnswered;

    await expect(page.locator('[data-testid="app-user-badge"]')).toContainText('alice');
    await expect(page.locator('[data-testid="app-reset-button"]')).toBeVisible();
    // Still signed in once the interface has settled.
    await page.waitForLoadState('networkidle');
    await expect(page.locator('[data-testid="app-user-badge"]')).toContainText('alice');
    await docsScreenshot(page, 'authentication-user-badge.png', '[data-testid="app-user-badge"]');
  });

  test('a file of the built bundle loads without a token', async ({ request }) => {
    const assetFiles = readdirSync(path.join(staticDir, 'assets'));
    const realFile = assetFiles.find((f) => f.endsWith('.js')) || assetFiles[0];
    const res = await request.get(`${baseUrl}/assets/${realFile}`);
    expect(res.status()).toBe(200);
  });

  test('a call to the management API without a token still gets 401', async ({ request }) => {
    // Opening the files of the interface opens nothing of the API: outside its four open routes (health, auth status,
    // login, token validation), it still asks for a token.
    const res = await request.get(`${baseUrl}/api/services`);
    expect(res.status()).toBe(401);
    const body = await res.json();
    expect(body.error).toBe('Missing token.');
  });
});
