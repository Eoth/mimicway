import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';
import http from 'node:http';

const API = 'http://localhost:7342/api';

// A real target whose answer depends on ?id=: two calls to the same endpoint get different answers, the case that
// suggestions exist for. It sends Content-Length: without it, Node answers chunked, and Mimicway does not capture a
// body of unknown size (ProxyClient::forward_with_capture, src/engine/proxy.rs).
function startFakeTarget() {
  return new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      const url = new URL(req.url, 'http://x');
      const found = url.searchParams.get('id') === '1';
      const body = found ? '{"found":true}' : '{"found":false}';
      res.writeHead(found ? 200 : 404, {
        'content-type': 'application/json',
        'content-length': Buffer.byteLength(body),
      });
      res.end(body);
    });
    server.listen(0, '127.0.0.1', () => resolve(server));
  });
}

function proxyService(name, targetPort) {
  return {
    name,
    listen_path: '/*',
    real_target_url: `http://127.0.0.1:${targetPort}`,
    is_mocked: false,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
  };
}

test.describe('Observed proxy traffic and suggested rules', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('observe a service, send it varied traffic, then get a suggestion and save it', async ({ page, request }) => {
    test.setTimeout(30000);
    const target = await startFakeTarget();
    const targetPort = target.address().port;

    try {
      const created = await request.post(`${API}/services`, {
        data: proxyService('orders-proxy', targetPort),
      });
      expect(created.ok()).toBe(true);

      await page.goto('/');
      await page.waitForLoadState('networkidle');
      await page.getByText('No group').click();
      await page.getByTestId('service-card-configure-button-orders-proxy').click();

      const panel = page.getByTestId('observation-panel-orders-proxy');
      await expect(panel).toBeVisible();
      const toggleBtn = page.getByTestId('observation-toggle-button-orders-proxy');
      await expect(toggleBtn).toHaveText('Observe this service');
      await docsScreenshot(page, 'observation-panel-off.png', '[data-testid="observation-panel-orders-proxy"]');

      await toggleBtn.click();
      await expect(toggleBtn).toHaveText('Stop observing');
      await docsScreenshot(page, 'observation-panel-on.png', '[data-testid="observation-panel-orders-proxy"]');

      // Real proxied traffic, id=1 and id=2 three times each: six samples of the route, above the three a suggestion
      // waits for (TRAFFIC_OBSERVATION_MIN_SAMPLES).
      for (let i = 0; i < 3; i++) {
        const r1 = await request.get('http://localhost:7342/orders-proxy/orders?id=1');
        expect(r1.status()).toBe(200);
        const r2 = await request.get('http://localhost:7342/orders-proxy/orders?id=2');
        expect(r2.status()).toBe(404);
      }

      await page.getByTestId('observation-refresh-suggestions-button-orders-proxy').click();
      const firstSuggestionCard = page.getByText('if Query parameter "id" = "1"');
      await expect(firstSuggestionCard).toBeVisible();
      await expect(page.getByText('if Query parameter "id" = "2"')).toBeVisible();
      await docsScreenshot(
        page,
        'observation-suggestions.png',
        '[data-testid="observation-suggestion-orders-proxy-0-0"], [data-testid="observation-suggestion-orders-proxy-0-1"]',
      );

      // Using a suggestion fills the rule form (method, sub-path, condition) and saves nothing: the user decides.
      await page.getByTestId('observation-use-suggestion-orders-proxy-0-0').click();
      await expect(page.getByTestId('rule-form-method-select')).toHaveValue('GET');
      await expect(page.getByTestId('rule-form-subpath-input')).toHaveValue('/orders');
      await docsScreenshot(
        page,
        'observation-suggestion-prefilled-form.png',
        '[data-testid="rule-form-method-select"], [data-testid="rule-form-edit-condition-allof-button-0"]',
      );

      await page.getByTestId('rule-form-name-input').fill('id-1-found');
      await page.getByTestId('rule-form-submit-button').click();

      await expect(page.getByTestId('rule-list-item-id-1-found')).toBeVisible();
      const rules = await request.get(`${API}/services/orders-proxy`);
      const savedRule = (await rules.json()).rules.find((r) => r.name === 'id-1-found');
      expect(savedRule.conditions.all_of).toEqual([
        { source: { type: 'QueryParam', key: 'id' }, operator: { type: 'Eq', value: '1' } },
      ]);
      expect(savedRule.response.status).toBe(200);
    } finally {
      target.close();
    }
  });

  test('answers that vary with nothing in the request get no suggested rule', async ({ page, request }) => {
    test.setTimeout(30000);
    // The same call gets a different answer each time: no rule can tell them apart, and freezing the first answer seen
    // would be wrong.
    let counter = 0;
    const target = await new Promise((resolve) => {
      const server = http.createServer((req, res) => {
        counter += 1;
        const body = `{"n":${counter}}`;
        res.writeHead(200, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) });
        res.end(body);
      });
      server.listen(0, '127.0.0.1', () => resolve(server));
    });
    const targetPort = target.address().port;

    try {
      const created = await request.post(`${API}/services`, {
        data: proxyService('flaky-proxy', targetPort),
      });
      expect(created.ok()).toBe(true);

      await request.post(`${API}/services/flaky-proxy/observe`);
      for (let i = 0; i < 3; i++) {
        await request.get('http://localhost:7342/flaky-proxy/status');
      }

      await page.goto('/');
      await page.waitForLoadState('networkidle');
      await page.getByText('No group').click();
      await page.getByTestId('service-card-configure-button-flaky-proxy').click();
      await page.getByTestId('observation-refresh-suggestions-button-flaky-proxy').click();

      await expect(page.getByText(/Varying responses observed/)).toBeVisible();
      await expect(page.getByTestId('observation-suggestion-flaky-proxy-0-0')).toHaveCount(0);
    } finally {
      target.close();
    }
  });
});
