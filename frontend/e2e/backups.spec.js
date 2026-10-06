import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const API = 'http://localhost:7342/api';

function validService(name, overrides = {}) {
  return {
    name,
    listen_path: '/v1/*',
    real_target_url: 'http://backend:8080',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
    ...overrides,
  };
}

async function backupFilenames(request) {
  const res = await request.get(`${API}/config/backups`);
  const backups = await res.json();
  return new Set(backups.map((b) => b.filename));
}

// The backup that one write creates, found by comparing the file names before and after it: no assumption on their
// order or their dates, whatever the earlier tests of the suite left in backups/.
async function newBackupFilename(request, action) {
  const before = await backupFilenames(request);
  await action();
  const after = await backupFilenames(request);
  const added = [...after].filter((f) => !before.has(f));
  expect(added.length).toBe(1);
  return added[0];
}

test.describe('Config backups & restore', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('GET /api/config/backups returns metadata only (no YAML content)', async ({ request }) => {
    await request.post(`${API}/services`, { data: validService('meta-svc') });
    const res = await request.get(`${API}/config/backups`);
    expect(res.ok()).toBe(true);
    const backups = await res.json();
    expect(Array.isArray(backups)).toBe(true);
    expect(backups.length).toBeGreaterThan(0);
    for (const b of backups) {
      expect(typeof b.filename).toBe('string');
      expect(typeof b.protected).toBe('boolean');
      expect(typeof b.size_bytes).toBe('number');
      expect(typeof b.created_at_ms).toBe('number');
    }
  });

  test('restore rejects path traversal filename (400)', async ({ request }) => {
    const res = await request.post(`${API}/config/restore/${encodeURIComponent('../mock-config.yaml')}`);
    expect(res.status()).toBe(400);
  });

  test('restore rejects unknown filename (404)', async ({ request }) => {
    const res = await request.post(`${API}/config/restore/mock-config-9999999999999-000042.yaml`);
    expect(res.status()).toBe(404);
  });

  // The refusal of a user who is not a super-admin (403, require_super_admin) cannot be reached here: without
  // authentication, every caller is an anonymous super-admin. The server's tests cover the refusal
  // (require_super_admin_rejects_non_admin, src/server/api.rs), and BackupManager.test.js how the interface reports it.

  test('UI restore flow: click through to a restored config', async ({ page, request }) => {
    const targetFilename = await newBackupFilename(request, () =>
      request.post(`${API}/services`, { data: validService('restore-target-svc') }),
    );

    // That backup holds the configuration from before the write, with no service. A second write changes the current
    // one, which the restore must undo.
    await request.post(`${API}/services`, { data: validService('restore-decoy-svc') });

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.getByText('Backups', { exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Configuration backups' })).toBeVisible();

    const row = page.locator('.backup-card', { hasText: targetFilename });
    await expect(row).toBeVisible();
    await docsScreenshot(page, 'backups-list.png', `[data-testid="backup-manager-item-${targetFilename}"]`);
    await row.getByText('Restore').click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await docsScreenshot(page, 'backups-restore-confirmation.png', '[data-testid="confirm-dialog"] .modal-content');
    await dialog.locator('#confirm-keyword-input').fill('RESTORE');
    await dialog.getByRole('button', { name: 'Restore' }).click();

    await expect(dialog).not.toBeVisible();

    await expect(async () => {
      const res = await request.get(`${API}/services`);
      const services = await res.json();
      expect(services.length).toBe(0);
    }).toPass();
  });

  test('UI restore flow: cancelling the confirmation does not change the config', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: validService('cancel-flow-svc') });

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.getByText('Backups', { exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Configuration backups' })).toBeVisible();

    const anyRow = page.locator('.backup-card').first();
    await expect(anyRow).toBeVisible();
    await anyRow.getByText('Restore').click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await dialog.getByText('Cancel').click();
    await expect(dialog).not.toBeVisible();

    const res = await request.get(`${API}/services`);
    const services = await res.json();
    expect(services.some((s) => s.name === 'cancel-flow-svc')).toBe(true);
  });
});
