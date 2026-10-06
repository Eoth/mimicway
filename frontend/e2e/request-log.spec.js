import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const ROOT = 'http://localhost:7342';
const API = `${ROOT}/api`;

// A mocked service whose only rule answers paid orders: one call matches it, the other one matches no rule.
const ordersService = {
  name: 'orders-api',
  listen_path: '/orders/{id}',
  real_target_url: '',
  is_mocked: true,
  rewrite_directory_urls: false,
  group_name: null,
  wsdl_mode: 'auto',
  rules: [
    {
      name: 'paid-order',
      method: 'GET',
      sub_path: null,
      action: 'mock',
      pre_script: null,
      script: null,
      post_script: null,
      conditions: {
        all_of: [{ source: { type: 'QueryParam', key: 'status' }, operator: { type: 'Eq', value: 'paid' } }],
        any_of: [],
      },
      response: {
        status: 200,
        headers: [{ name: 'Content-Type', value: 'application/json' }],
        body: [{ type: 'Template', template: '{"id":"{{path.id}}","status":"paid"}' }],
        chaos: null,
      },
    },
  ],
};

test.describe('Request log', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    expect((await request.post(`${API}/services`, { data: ordersService })).ok()).toBe(true);
    expect((await request.get(`${ROOT}/orders-api/orders/42?status=paid`)).status()).toBe(200);
    expect((await request.get(`${ROOT}/orders-api/orders/43`)).status()).toBe(404);
  });

  test('lists the calls a service received and shows what matched them', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="app-nav-logs-button"]').click();
    // The log is kept in memory across configuration resets: keep this service's calls only.
    await page.locator('[data-testid="request-log-filter-service"]').selectOption('orders-api');

    // Newest first: the unmatched call, then the one the rule answered.
    const unmatched = page.locator('[data-testid="request-log-row-0"]');
    const matched = page.locator('[data-testid="request-log-row-1"]');
    await expect(unmatched).toContainText('/orders-api/orders/43');
    await expect(matched).toContainText('/orders-api/orders/42');
    await expect(matched).toContainText('paid-order');
    await docsScreenshot(
      page,
      'request-log-list.png',
      '[data-testid="request-log-row-0"], [data-testid="request-log-row-1"]',
    );

    await page.locator('[data-testid="request-log-detail-button-1"]').click();
    const detail = page.locator('[data-testid="request-log-detail-modal"]');
    await expect(detail).toBeVisible();
    await expect(detail).toContainText('paid-order');
    await expect(detail).toContainText('/orders-api/orders/42');
    await expect(detail).toContainText('200');
    await docsScreenshot(page, 'request-log-detail.png', '[data-testid="request-log-detail-modal"] .modal-content');
  });
});
