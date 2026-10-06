import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const API = 'http://localhost:7342/api';

function validService(name, overrides = {}) {
  return {
    name,
    listen_path: '',
    real_target_url: 'http://backend:8080',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
    ...overrides,
  };
}

test.beforeEach(async ({ request }) => {
  await request.delete(`${API}/config/reset`);
});

test('the example service answers with its path parameter', async ({ page, request }) => {
  await page.goto('/');
  await page.waitForLoadState('networkidle');
  await page.getByRole('button', { name: /Load an example/ }).click();

  // The example is created through the API: its service answers once that call is done.
  const call = () => request.get('http://localhost:7342/users-api/users/42');
  await expect.poll(async () => (await call()).status()).toBe(200);
  const json = await (await call()).json();
  expect(json.id).toBe(42);
  expect(json.name).toBeTruthy();
  expect(json.meta.timestamp).toBeGreaterThan(1700000000000);
});

test('export downloads a valid JSON file', async ({ page, request }) => {
  await request.post(`${API}/services`, { data: validService('export-test') });

  await page.goto('/');
  await page.waitForLoadState('networkidle');
  await docsScreenshot(page, 'administration-export-button.png', '[data-testid="app-export-button"]');

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('button', { name: 'Export', exact: true }).click(),
  ]);

  const content = await (await download.createReadStream()).toArray();
  const text = Buffer.concat(content).toString('utf-8');
  const config = JSON.parse(text);
  expect(config.services).toBeInstanceOf(Array);
  expect(config.services.some((s) => s.name === 'export-test')).toBe(true);
});

test('a configuration is imported through the API', async ({ request }) => {
  const config = {
    services: [validService('imported-svc')],
    groups: [],
  };
  const res = await request.put(`${API}/config`, { data: config });
  expect(res.status()).toBe(200);

  const services = await (await request.get(`${API}/services`)).json();
  expect(services.length).toBe(1);
  expect(services[0].name).toBe('imported-svc');
});

test('import asks whether to replace everything or merge, and merging keeps what exists', async ({ page, request }) => {
  // The file to import is a real export: a group and a service of that group.
  await request.post(`${API}/groups`, { data: { name: 'payments', code: '', admins: [], members: [] } });
  await request.post(`${API}/services`, { data: validService('card-api', { group_name: 'payments' }) });
  const exported = await (await request.get(`${API}/config`)).json();
  await request.delete(`${API}/config/reset`);
  await request.post(`${API}/services`, { data: validService('existing-svc') });

  await page.goto('/');
  await page.waitForLoadState('networkidle');
  await page.getByTestId('app-import-file-input').setInputFiles({
    name: 'mimicway-config.json',
    mimeType: 'application/json',
    buffer: Buffer.from(JSON.stringify(exported)),
  });
  const modal = page.getByTestId('app-import-modal');
  await expect(modal).toContainText('1 service(s) and 1 group(s) found in the file.');
  await expect(page.getByTestId('app-import-replace-button')).toBeVisible();
  await expect(page.getByTestId('app-import-merge-button')).toBeVisible();
  await docsScreenshot(page, 'administration-import-dialog.png', '[data-testid="app-import-modal"] .modal-content');

  await page.getByTestId('app-import-merge-button').click();
  await expect(modal).toBeHidden();
  await expect(page.getByTestId('notification')).toContainText('1 service(s) and 1 group(s) added');
  const services = await (await request.get(`${API}/services`)).json();
  expect(services.map((s) => [s.name, s.group_name]).sort()).toEqual([
    ['card-api', 'payments'],
    ['existing-svc', null],
  ]);
  const groups = await (await request.get(`${API}/groups`)).json();
  expect(groups.map((g) => g.name)).toEqual(['payments']);
});

test('reset unlocks only once its keyword is typed, then removes every service', async ({ page, request }) => {
  await request.post(`${API}/services`, { data: validService('doomed-svc') });
  // The shared instance runs with the reset button hidden, its default (SHOW_RESET_BUTTON): the status is answered as
  // an instance that shows it. The reset that follows is the real one.
  await page.route('**/api/auth/status', (route) =>
    route.fulfill({ json: { enabled: false, show_reset_button: true } }),
  );

  await page.goto('/');
  await page.waitForLoadState('networkidle');
  await page.getByTestId('app-reset-button').click();
  const keyword = page.getByTestId('confirm-dialog-keyword-input');
  const confirm = page.getByTestId('confirm-dialog-confirm-button');
  await expect(confirm).toBeDisabled();
  await keyword.fill('reset');
  await expect(confirm).toBeDisabled();
  await keyword.fill('RESET');
  await expect(confirm).toBeEnabled();
  await docsScreenshot(page, 'administration-reset-confirmation.png', '[data-testid="confirm-dialog"] .modal-content');

  await confirm.click();
  await expect(page.getByTestId('confirm-dialog')).toBeHidden();
  await expect.poll(async () => (await (await request.get(`${API}/services`)).json()).length).toBe(0);
});
