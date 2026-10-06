import { test, expect } from '@playwright/test';

const API = 'http://localhost:7342/api';

// On a 360 px screen (a phone held upright) no main screen may scroll sideways, in either theme: long URLs break, a
// wide table scrolls inside its own box, and fields shrink with their form.
const rule = (name, method, action) => ({
  name,
  method,
  sub_path: null,
  action,
  pre_script: null,
  script: null,
  post_script: null,
  conditions: { all_of: [], any_of: [] },
  response: {
    status: 200,
    headers: [{ name: 'Content-Type', value: 'application/json' }],
    body: [{ type: 'Literal', value: '{"ok":true}' }],
    chaos: null,
  },
});

const screens = {
  'the service list': async () => {},
  'a service page': async (page) => {
    await page.getByRole('button', { name: /Configure the service narrow-svc/ }).click();
    await expect(page.getByTestId('rule-list-item-narrow-get')).toBeVisible();
  },
  'the rule form': async (page) => {
    await page.getByRole('button', { name: /Configure the service narrow-svc/ }).click();
    await page.getByTestId('rule-list-edit-button-narrow-get').click();
    await expect(page.getByTestId('rule-form-name-input')).toBeVisible();
  },
  'the request log': async (page) => {
    await page.getByTestId('app-nav-logs-button').click();
    await expect(page.locator('table.log-table')).toBeVisible();
  },
  'the groups': async (page) => {
    await page.getByTestId('app-nav-groups-button').click();
    await expect(page.getByRole('heading', { name: 'Service groups' })).toBeVisible();
  },
};

test.describe('Narrow screens: nothing scrolls sideways at 360 px', () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: {
        name: 'narrow-svc',
        listen_path: '/customers/{id}/orders',
        real_target_url: 'http://a-rather-long-backend-name.internal.example:8080',
        is_mocked: true,
        rewrite_directory_urls: false,
        group_name: null,
        wsdl_mode: 'auto',
        rules: [rule('narrow-get', 'GET', 'mock'), rule('narrow-proxy', 'GET', 'proxy')],
      },
    });
    // Traffic, so that the request log shows its table.
    await request.get('http://localhost:7342/narrow-svc/customers/42/orders');
  });

  for (const theme of ['light', 'dark']) {
    for (const [name, reach] of Object.entries(screens)) {
      test(`${name}, ${theme} theme`, async ({ page }) => {
        await page.addInitScript((t) => localStorage.setItem('mimicway-theme', t), theme);
        await page.goto('/');
        const group = page.locator('button[aria-expanded]').first();
        if ((await group.getAttribute('aria-expanded')) === 'false') await group.click();
        await reach(page);
        const widths = await page.evaluate(() => [document.documentElement.scrollWidth, window.innerWidth]);
        expect(widths[0], `page ${widths[0]} px wide in a ${widths[1]} px window`).toBeLessThanOrEqual(widths[1]);
      });
    }
  }
});
