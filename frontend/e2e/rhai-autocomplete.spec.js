import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const API = 'http://localhost:7342/api';

function validService(name, overrides = {}) {
  return {
    name,
    listen_path: '/e2e/*',
    real_target_url: 'http://e2e:80',
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
  await request.post(`${API}/services`, { data: validService('autocomplete-svc') });
});

async function openAddRuleForm(page) {
  await page.goto('/');
  await page.waitForLoadState('networkidle');
  const group = page.locator('button[aria-expanded]').first();
  if ((await group.getAttribute('aria-expanded')) === 'false') {
    await group.click();
  }
  await page
    .getByRole('button', { name: /Configure/ })
    .first()
    .click();
  await page.getByRole('button', { name: /Add a rule/ }).click();
}

test('autocompletion: a click inserts the function with its parameters', async ({ page }) => {
  await openAddRuleForm(page);
  await page.locator('input#rule-name').fill('ac-rule-click');
  await page.getByRole('switch', { name: 'Custom script' }).click();

  const scriptField = page.locator('#rule-script');
  await scriptField.click();
  await scriptField.type('seed');

  const option = page.getByRole('option', { name: /seeded_pick/ });
  await expect(option).toBeVisible();
  await docsScreenshot(
    page,
    'rhai-autocompletion.png',
    '[data-testid="rhai-script-editor-textarea-rule-script"], [data-testid="rhai-script-editor-suggestions-rule-script"]',
  );
  await option.click();

  await expect(scriptField).toHaveValue('seeded_pick(seed, ["a", "b"])');
  await expect(page.getByRole('listbox')).not.toBeVisible();
});

test('autocompletion: Enter inserts the highlighted function', async ({ page }) => {
  await openAddRuleForm(page);
  await page.locator('input#rule-name').fill('ac-rule-kbd');
  await page.getByRole('switch', { name: 'Custom script' }).click();

  const scriptField = page.locator('#rule-script');
  await scriptField.click();
  await scriptField.type('date_n');

  await expect(page.getByRole('listbox')).toBeVisible();
  await scriptField.press('Enter');

  await expect(scriptField).toHaveValue('date_now("iso")');
});

test('autocompletion: Escape closes the list and inserts nothing', async ({ page }) => {
  await openAddRuleForm(page);
  await page.locator('input#rule-name').fill('ac-rule-esc');
  await page.getByRole('switch', { name: 'Custom script' }).click();

  const scriptField = page.locator('#rule-script');
  await scriptField.click();
  await scriptField.type('uuid');
  await expect(page.getByRole('listbox')).toBeVisible();

  await scriptField.press('Escape');
  await expect(page.getByRole('listbox')).not.toBeVisible();
  await expect(scriptField).toHaveValue('uuid');
});

test('autocompletion: Ctrl+Space opens the list with nothing typed', async ({ page }) => {
  await openAddRuleForm(page);
  await page.locator('input#rule-name').fill('ac-rule-ctrlspace');
  await page.getByRole('switch', { name: 'Custom script' }).click();

  const scriptField = page.locator('#rule-script');
  await scriptField.click();
  await scriptField.press('Control+ ');

  const options = page.getByRole('option');
  await expect(options).not.toHaveCount(0);
  await expect(await options.count()).toBeGreaterThan(5);
});

test('a function inserted by autocompletion is saved as inserted', async ({ page, request }) => {
  await openAddRuleForm(page);
  await page.locator('input#rule-name').fill('ac-rule-persist');
  await page.getByRole('switch', { name: 'Custom script' }).click();

  const scriptField = page.locator('#rule-script');
  await scriptField.click();
  await scriptField.type('seeded_int');
  const option = page.getByRole('option', { name: /^seeded_int/ });
  await option.click();
  // The suggested parameters come selected: typing replaces them.
  await page.keyboard.type('request.path.id, 0, 10');

  await page.getByRole('button', { name: /Add the rule/ }).click();

  // The rule is saved through the API: wait for that call rather than for a fixed time.
  await expect
    .poll(async () => {
      const svc = await (await request.get(`${API}/services/autocomplete-svc`)).json();
      return svc.rules.find((r) => r.name === 'ac-rule-persist')?.script;
    })
    .toBe('seeded_int(request.path.id, 0, 10)');
});
