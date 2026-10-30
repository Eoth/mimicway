import { test, expect } from '@playwright/test';

const API = 'http://localhost:7342/api';
const BASE = 'http://localhost:7342';

// The help on templates in the response builders, proven by the answer of the mock itself: a pipe picked from the
// list for a kept value, and a free text whose value of the request is picked after `{{`, both rendered by the engine
// on a real request.

const shop = {
  name: 'shop',
  listen_path: '',
  real_target_url: '',
  is_mocked: true,
  rewrite_directory_urls: false,
  group_name: null,
  wsdl_mode: 'auto',
  rules: [],
};

test.describe('Help on templates in the response builders', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, { data: shop });
  });

  test('a pipe on a kept value and a value picked after {{ reach the real answer', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    const group = page.locator('button[aria-expanded]').first();
    if ((await group.count()) > 0 && (await group.getAttribute('aria-expanded')) === 'false') await group.click();
    await page.getByRole('button', { name: /Configure the service shop/ }).click();
    await page.getByRole('button', { name: /Add a rule/ }).click();
    await page.getByTestId('rule-form-name-input').fill('order');
    await page.getByTestId('rule-form-subpath-input').fill('/orders/{ref}');
    await page.getByTestId('rule-form-mode-button-json').click();
    await page.getByTestId('json-paste-builder-textarea').fill('{"name":"ada","id":"A1"}');
    await page.getByTestId('json-paste-builder-analyze-button').click();

    const pipe = page.getByTestId('json-paste-builder-pipe-input-0');
    await pipe.click();
    await expect(page.getByTestId('json-paste-builder-pipe-input-0-suggestions')).toBeVisible();
    await pipe.pressSequentially('up');
    await pipe.press('Enter');
    await expect(pipe).toHaveValue('upper');
    await expect(page.getByTestId('json-paste-builder-pipe-input-0-tip')).toContainText('“AbC” → “ABC”');

    const value = page.getByTestId('json-paste-builder-value-input-1');
    await value.fill('');
    await value.pressSequentially('Order {{pa');
    await page.getByTestId('json-paste-builder-value-input-1-suggestion-path.').click();
    await value.pressSequentially('ref');
    await value.press('End');
    await expect(value).toHaveValue('Order {{path.ref');
    await expect(page.getByTestId('json-paste-builder-value-input-1-notes')).toContainText('closes');
    await value.pressSequentially('}}');
    await expect(page.getByTestId('json-paste-builder-value-input-1-notes')).toHaveCount(0);

    await page.getByTestId('rule-form-submit-button').click();
    await expect(page.getByTestId('rule-list-edit-button-order')).toBeVisible();

    const response = await fetch(`${BASE}/shop/orders/B7`);
    expect(await response.json()).toEqual({ name: 'ADA', id: 'Order B7' });
  });
});
