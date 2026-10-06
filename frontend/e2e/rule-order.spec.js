// The order of a service's rules decides which one answers: the first match wins. Reordering them by drag and drop
// changes what the service returns, which is what this checks, through the list, the API and a real call.
import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const API = 'http://localhost:7342/api';
const ORDER_42 = 'http://localhost:7342/orders-demo/orders/42';

function textRule(name, text, conditions = []) {
  return {
    name,
    method: 'GET',
    sub_path: null,
    action: 'mock',
    pre_script: null,
    script: null,
    post_script: null,
    conditions: { all_of: conditions, any_of: [] },
    response: {
      status: 200,
      headers: [{ name: 'Content-Type', value: 'text/plain' }],
      body: [{ type: 'Literal', value: text }],
      chaos: null,
    },
  };
}

test.describe('Rule order', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    // A general rule placed before a specific one hides it, the mistake the guide warns about.
    const created = await request.post(`${API}/services`, {
      data: {
        name: 'orders-demo',
        listen_path: '/orders/{id}',
        real_target_url: '',
        is_mocked: true,
        rewrite_directory_urls: false,
        group_name: null,
        wsdl_mode: 'auto',
        rules: [
          textRule('any-order', 'any order'),
          textRule('order-42', 'order 42', [
            { source: { type: 'PathParam', key: 'id' }, operator: { type: 'Eq', value: '42' } },
          ]),
        ],
      },
    });
    expect(created.ok()).toBe(true);
  });

  test('dragging a rule above another one makes it match first', async ({ page, request }) => {
    expect(await (await request.get(ORDER_42)).text()).toBe('any order');

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.getByTestId('service-group-header-ungrouped').click();
    await page.getByTestId('service-card-configure-button-orders-demo').click();
    const reordered = page.waitForResponse((r) => r.url().endsWith('/rules/reorder') && r.request().method() === 'PUT');
    await page.getByTestId('rule-list-grip-order-42').dragTo(page.getByTestId('rule-list-grip-any-order'));
    expect((await reordered).ok()).toBe(true);

    // The list shows the new order, numbered from the top.
    await expect(page.locator('[data-testid^="rule-list-item-"] .rule-name')).toHaveText(['order-42', 'any-order']);
    await expect(page.getByTestId('rule-list-item-order-42').locator('.rule-index')).toHaveText('1');
    await expect(page.getByTestId('notification')).toBeHidden();
    await docsScreenshot(page, 'rule-list-reordered.png', '[data-testid^="rule-list-item-"]');

    const services = await (await request.get(`${API}/services`)).json();
    expect(services.find((s) => s.name === 'orders-demo').rules.map((r) => r.name)).toEqual(['order-42', 'any-order']);
    expect(await (await request.get(ORDER_42)).text()).toBe('order 42');
    expect(await (await request.get('http://localhost:7342/orders-demo/orders/7')).text()).toBe('any order');
  });
});
