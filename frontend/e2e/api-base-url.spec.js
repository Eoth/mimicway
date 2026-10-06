import { test, expect } from '@playwright/test';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// The interface calls the API on the origin that served it (relative /api/... paths), unless API_BASE_URL is set on
// the process that serves it, which tells the page another base through /runtime-config.json: for an infrastructure
// that routes /api to another origin than the static files. Both cases are checked.
//
// This file starts its own instances of Mimicway, never the shared one on :7342: two of them when front and back stand
// on different origins. It needs the binary built (`cargo build`) and frontend/dist, like the rest of the suite.

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
  throw new Error(`Mimicway (instance started by this spec) did not start in time on ${baseUrl}: ${lastError}`);
}

async function spawnMimicway({ port, dataDir, extraEnv = {} }) {
  const baseUrl = `http://127.0.0.1:${port}`;
  const child = spawn(binaryPath, [], {
    cwd: repoRoot,
    env: {
      ...process.env,
      PORT: String(port),
      STATIC_DIR: staticDir,
      DATA_PATH: dataDir,
      AUTH_ENABLED: 'false',
      ...extraEnv,
    },
    stdio: 'pipe',
  });
  await waitForHealth(baseUrl);
  return { child, baseUrl };
}

test.describe('API base URL apart from the origin of the interface', () => {
  test('by default, /runtime-config.json gives an empty base and the interface uses its own origin', async ({
    page,
  }) => {
    const dataDir = mkdtempSync(path.join(tmpdir(), 'mimicway-apibase-default-'));
    const port = await getFreePort();
    let child;
    try {
      ({ child } = await spawnMimicway({ port, dataDir }));
      const baseUrl = `http://127.0.0.1:${port}`;

      const configRes = await page.request.get(`${baseUrl}/runtime-config.json`);
      expect(configRes.status()).toBe(200);
      expect((await configRes.json()).api_base_url).toBe('');

      await page.goto(baseUrl + '/');
      // The list header and its add button show once start-up is over, start-up including the relative /api/...
      // calls.
      await expect(page.getByText('Mimicway')).toBeVisible();
      await expect(page.locator('[data-testid="app-add-service-button"]')).toBeVisible();
    } finally {
      if (child) child.kill();
      rmSync(dataDir, { recursive: true, force: true });
    }
  });

  test('with front and back on different origins, the interface calls the API at API_BASE_URL', async ({ page }) => {
    const backDataDir = mkdtempSync(path.join(tmpdir(), 'mimicway-apibase-back-'));
    const frontDataDir = mkdtempSync(path.join(tmpdir(), 'mimicway-apibase-front-'));
    const backPort = await getFreePort();
    let frontPort = await getFreePort();
    if (frontPort === backPort) frontPort = await getFreePort();

    let backChild;
    let frontChild;
    try {
      // The API answers a browser on another origin only if that origin is listed.
      ({ child: backChild } = await spawnMimicway({
        port: backPort,
        dataDir: backDataDir,
        extraEnv: { CORS_ALLOWED_ORIGINS: `http://127.0.0.1:${frontPort}` },
      }));
      const backBaseUrl = `http://127.0.0.1:${backPort}`;

      // The service exists on the back only, created there directly: the front, whose own store stays empty, can show
      // it only by calling this API.
      const createRes = await page.request.post(`${backBaseUrl}/api/services`, {
        data: {
          name: 'cross-origin-demo',
          listen_path: '/v1/*',
          real_target_url: '',
          is_mocked: true,
          rewrite_directory_urls: false,
          group_name: null,
          wsdl_mode: 'auto',
          rules: [],
        },
      });
      expect(createRes.status()).toBe(201);

      ({ child: frontChild } = await spawnMimicway({
        port: frontPort,
        dataDir: frontDataDir,
        extraEnv: { API_BASE_URL: backBaseUrl },
      }));
      const frontBaseUrl = `http://127.0.0.1:${frontPort}`;

      const configRes = await page.request.get(`${frontBaseUrl}/runtime-config.json`);
      expect(configRes.status()).toBe(200);
      expect((await configRes.json()).api_base_url).toBe(backBaseUrl);

      await page.goto(frontBaseUrl + '/');
      // Groups start collapsed, the ungrouped one too.
      await page.locator('[data-testid="service-group-header-ungrouped"]').click();
      await expect(page.locator('[data-testid="service-card-cross-origin-demo"]')).toBeVisible();

      // The front's own store never got the service.
      const frontOwnServices = await page.request.get(`${frontBaseUrl}/api/services`);
      const frontOwnBody = await frontOwnServices.json();
      expect(frontOwnBody.find((s) => s.name === 'cross-origin-demo')).toBeUndefined();
    } finally {
      if (frontChild) frontChild.kill();
      if (backChild) backChild.kill();
      rmSync(frontDataDir, { recursive: true, force: true });
      rmSync(backDataDir, { recursive: true, force: true });
    }
  });

  test('an origin missing from CORS_ALLOWED_ORIGINS cannot call the management API from a browser', async ({
    page,
  }) => {
    const backDataDir = mkdtempSync(path.join(tmpdir(), 'mimicway-apibase-cors-back-'));
    const frontDataDir = mkdtempSync(path.join(tmpdir(), 'mimicway-apibase-cors-front-'));
    const backPort = await getFreePort();
    let frontPort = await getFreePort();
    if (frontPort === backPort) frontPort = await getFreePort();

    let backChild;
    let frontChild;
    try {
      ({ child: backChild } = await spawnMimicway({ port: backPort, dataDir: backDataDir }));
      const backBaseUrl = `http://127.0.0.1:${backPort}`;
      ({ child: frontChild } = await spawnMimicway({ port: frontPort, dataDir: frontDataDir }));
      await page.goto(`http://127.0.0.1:${frontPort}/`);

      const outcome = await page.evaluate(async (url) => {
        try {
          await fetch(`${url}/api/config`);
          return 'readable';
        } catch {
          return 'blocked';
        }
      }, backBaseUrl);
      expect(outcome).toBe('blocked');
    } finally {
      if (frontChild) frontChild.kill();
      if (backChild) backChild.kill();
      rmSync(frontDataDir, { recursive: true, force: true });
      rmSync(backDataDir, { recursive: true, force: true });
    }
  });
});
