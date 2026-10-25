import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test, expect } from '@playwright/test';
import { docsScreenshot, DOCS_SCREENSHOTS_ENABLED } from './docs-screenshot.js';

// The demo at the top of the README: the screens a first mock goes through, played on the real binary. Each step
// checks the state its screen shows and frames it like a guide image, so CI fails when a change of the interface
// breaks the demo; with DOCS_SCREENSHOTS set, each step also writes a frame per language into
// test-results/demo-tour/<language>/, with the step's caption in captions.json, and scripts/make-demo-tour.py turns
// them into docs/<language>/screenshots/demo-tour.webp.

const ROOT = 'http://localhost:7342';
// The address the frames show, a default installation's, whatever port this run uses.
const SHOWN_ORIGIN = 'http://localhost:7342';
const API = `${ROOT}/api`;
const FRAMES = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'test-results', 'demo-tour');

// Never contacted: example.com is reserved for documentation, the availability check only runs on a click, and the
// demo forwards nothing.
const services = [
  {
    name: 'users',
    listen_path: '/{id}',
    real_target_url: 'https://api.example.com/users',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
  },
  {
    name: 'payments',
    listen_path: '',
    real_target_url: 'https://payments.example.com',
    is_mocked: false,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
  },
];

const captions = [];
const testId = (id) => `[data-testid="${id}"]`;

// The subject outlined, as a presenter would point at it; only in the page of the test, never in the interface.
async function frame(page, caption, subject) {
  // A notification keeps the language it appeared in: the frame waits until it closes, as a viewer would.
  await expect(page.locator(testId('notification'))).toBeHidden({ timeout: 10_000 });
  captions.push(caption);
  const name = `${String(captions.length).padStart(2, '0')}.png`;
  const outline = await page.addStyleTag({
    content: `${subject} { outline: 3px solid #b45309 !important; outline-offset: 3px !important; }`,
  });
  await docsScreenshot(page, name, subject, { directory: FRAMES });
  await outline.evaluate((style) => style.remove());
}

test('the demo of the README goes through a first mock, screen by screen', async ({ page, request }) => {
  test.setTimeout(60_000);
  await request.delete(`${API}/config/reset`);
  for (const service of services) {
    expect((await request.post(`${API}/services`, { data: service })).ok()).toBe(true);
  }
  // A call the application makes before any rule exists: the rule tester replays it below.
  expect((await request.get(`${ROOT}/users/42?plan=vip`)).status()).toBe(404);

  await page.goto('/');
  const group = page.locator(testId('service-group-header-ungrouped'));
  if ((await group.getAttribute('aria-expanded')) === 'false') await group.click();
  await expect(page.locator(testId('service-card-users'))).toBeVisible();
  await expect(page.locator(testId('service-card-payments'))).toBeVisible();
  await frame(
    page,
    {
      en: 'One service per API your app calls, mocked or real',
      fr: 'Un service par API qu’appelle votre application, simulée ou réelle',
    },
    `${testId('service-card-users')}, ${testId('service-card-payments')}`,
  );

  await page.locator(testId('service-card-configure-button-users')).click();
  const testUrl = page.locator(testId('service-detail-test-url'));
  await expect(testUrl).toContainText('/users/');
  await testUrl.evaluate((code, shown) => {
    code.textContent = code.textContent.replace(window.location.origin, shown);
  }, SHOWN_ORIGIN);
  await expect(testUrl).toHaveText(`${SHOWN_ORIGIN}/users/{id}`);
  await frame(
    page,
    {
      en: 'Your app calls this URL instead of the real API',
      fr: 'Votre application appelle cette URL au lieu de la vraie API',
    },
    testId('service-detail-test-url'),
  );

  await page.locator(testId('rule-list-add-button')).click();
  await page.locator(testId('rule-form-name-input')).fill('vip');
  await page.locator(testId('rule-form-method-select')).selectOption('GET');
  await page.locator(testId('rule-form-add-condition-allof-button')).click();
  await page.locator(testId('condition-form-source-select')).selectOption('QueryParam');
  await page.locator(testId('condition-form-key-input')).fill('plan');
  await page.locator(testId('condition-form-value-input')).fill('vip');
  await page.locator(testId('condition-form-submit-button')).click();
  await expect(page.locator(testId('rule-form-edit-condition-allof-button-0'))).toHaveText('QueryParam(plan) Eq(vip)');
  await frame(
    page,
    { en: 'A condition picks the rule from the request', fr: 'Une condition choisit la règle selon la requête' },
    testId('rule-form-conditions-allof'),
  );

  const logSelect = page.locator('#rule-tester-log');
  const call = logSelect.locator('option', { hasText: 'GET /users/42' }).first();
  await logSelect.selectOption(await call.getAttribute('value'));
  await page.locator(testId('rule-tester-test-button')).click();
  await expect(page.locator(testId('rule-tester-result'))).toContainText(/would match this request/);
  await frame(
    page,
    {
      en: 'Try the rule on a call your app really made',
      fr: 'Essayez la règle sur un appel réel de votre application',
    },
    testId('rule-tester-result'),
  );

  await page.locator(testId('rule-form-mode-button-text')).click();
  await page.locator(testId('rule-form-text-content-textarea')).fill('VIP offer');
  await page.locator(testId('rule-form-submit-button')).click();
  await expect(page.locator(testId('rule-list-item-vip'))).toBeVisible();

  await page.locator(testId('rule-list-add-button')).click();
  await page.locator(testId('rule-form-name-input')).fill('customer');
  await page.locator(testId('rule-form-method-select')).selectOption('GET');
  await page.locator(testId('rule-form-mode-button-json')).click();
  await page.locator(testId('json-paste-builder-textarea')).fill('{"id":"42","name":"Ada","city":"Lyon"}');
  await page.locator(testId('json-paste-builder-analyze-button')).click();
  await page.locator(testId('json-paste-builder-source-select-1')).selectOption('fake');
  await page.locator(testId('json-paste-builder-fake-select-1')).selectOption('FirstName');
  await page.locator(testId('json-paste-builder-source-select-2')).selectOption('fake');
  await page.locator(testId('json-paste-builder-fake-select-2')).selectOption('CityFR');
  await frame(
    page,
    {
      en: 'Paste an example answer, choose the fake data',
      fr: 'Collez une réponse d’exemple, choisissez les fausses données',
    },
    [1, 2]
      .map(
        (i) => `${testId(`json-paste-builder-source-select-${i}`)}, ${testId(`json-paste-builder-fake-select-${i}`)}`,
      )
      .join(', '),
  );
  await page.locator(testId('rule-form-submit-button')).click();
  // It overlaps vip, which comes first: the general answer is the fallback of the specific one, as intended.
  await expect(page.locator(testId('rule-form-conflict-warning-item-vip'))).toBeVisible();
  await page.locator(testId('rule-form-conflict-save-anyway-button')).click();
  await expect(page.locator(testId('rule-list-item-customer'))).toBeVisible();

  await page.locator(testId('rule-list-edit-button-vip')).click();
  await page.locator(testId('rule-form-action-proxy-option')).click();
  await expect(page.locator(testId('rule-form-action-proxy-radio'))).toBeChecked();
  await frame(
    page,
    { en: 'Or let a rule through to the real API', fr: 'Ou laissez passer une règle vers la vraie API' },
    `${testId('rule-form-action-mock-option')}, ${testId('rule-form-action-proxy-option')}`,
  );
  await page.locator(testId('rule-form-cancel-button')).click();

  // The application's calls: the first rule that matches answers, the condition first.
  const vip = await request.get(`${ROOT}/users/42?plan=vip`);
  expect(await vip.text()).toBe('VIP offer');
  const customer = await request.get(`${ROOT}/users/7`);
  expect(await customer.json()).toMatchObject({ id: '42', name: expect.any(String), city: expect.any(String) });
  await page.locator(testId('app-nav-logs-button')).click();
  // The filter by service shows once the log holds more than one: in the full suite, earlier specs' calls are there.
  const byService = page.locator(testId('request-log-filter-service'));
  await expect(page.locator(testId('request-log-row-0'))).toBeVisible();
  if ((await byService.count()) > 0) await byService.selectOption('users');
  await expect(page.locator(testId('request-log-row-0'))).toContainText('customer');
  await expect(page.locator(testId('request-log-row-1'))).toContainText('vip');
  await frame(
    page,
    {
      en: 'Every call is logged, with the rule that answered it',
      fr: 'Chaque appel est journalisé, avec la règle qui y a répondu',
    },
    [0, 1, 2].map((i) => testId(`request-log-row-${i}`)).join(', '),
  );

  if (DOCS_SCREENSHOTS_ENABLED) {
    fs.writeFileSync(path.join(FRAMES, 'captions.json'), `${JSON.stringify(captions, null, 2)}\n`);
  }
});
