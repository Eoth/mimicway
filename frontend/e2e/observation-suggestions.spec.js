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

  test('names called once each through a directory give one rule that answers any name', async ({ page, request }) => {
    test.setTimeout(30000);
    // A service directory: /service/<hash>/<name> answers the address of the named service. Each name is called once:
    // the route pools the calls, and since the answers differ only by the name they repeat, one rule covers them all.
    const target = await new Promise((resolve) => {
      const server = http.createServer((req, res) => {
        const name = req.url.split('/').pop();
        const body = `<lookup><url>http://backend.internal:8080/ws/${name}</url></lookup>`;
        res.writeHead(200, { 'content-type': 'text/xml', 'content-length': Buffer.byteLength(body) });
        res.end(body);
      });
      server.listen(0, '127.0.0.1', () => resolve(server));
    });
    const targetPort = target.address().port;

    try {
      const created = await request.post(`${API}/services`, { data: proxyService('directory', targetPort) });
      expect(created.ok()).toBe(true);
      await request.post(`${API}/services/directory/observe`);
      for (const name of ['restOrders', 'restStock', 'restBilling']) {
        const r = await request.get(`http://localhost:7342/directory/service/9f86d081884c7d65/${name}`);
        expect(r.status()).toBe(200);
      }

      await page.goto('/');
      await page.waitForLoadState('networkidle');
      await page.getByText('No group').click();
      await page.getByTestId('service-card-configure-button-directory').click();
      await page.getByTestId('observation-refresh-suggestions-button-directory').click();

      await expect(page.getByTestId('observation-suggestion-paths-directory-0')).toHaveText(
        'GET /service/{hash}/{name} groups 3 observed paths, such as /service/9f86d081884c7d65/restBilling, ' +
          '/service/9f86d081884c7d65/restOrders, /service/9f86d081884c7d65/restStock',
      );
      const card = page.getByTestId('observation-suggestion-directory-0-0');
      await expect(card).toContainText('no condition: the response repeats Path parameter "name" (3 calls)');
      await expect(card).toContainText('<url>http://backend.internal:8080/ws/{{path.name}}</url>');
      await expect(page.getByTestId('observation-suggestion-directory-0-1')).toHaveCount(0);
      await docsScreenshot(
        page,
        'observation-route-suggestion.png',
        '[data-testid="observation-suggestion-paths-directory-0"], [data-testid="observation-suggestion-directory-0-0"]',
      );

      await page.getByTestId('observation-use-suggestion-directory-0-0').click();
      await expect(page.getByTestId('rule-form-subpath-input')).toHaveValue('/service/{hash}/{name}');
      await page.getByTestId('rule-form-name-input').fill('any-service');
      await page.getByTestId('rule-form-submit-button').click();
      await expect(page.getByTestId('rule-list-item-any-service')).toBeVisible();

      // Mocked, with no backend left: a name never observed gets its own address.
      const saved = await (await request.get(`${API}/services/directory`)).json();
      const updated = await request.put(`${API}/services/directory`, {
        data: { ...saved, is_mocked: true, real_target_url: '' },
      });
      expect(updated.ok()).toBe(true);
      const answer = await request.get('http://localhost:7342/directory/service/0123456789abcdef/restInvoices');
      expect(answer.status()).toBe(200);
      expect(await answer.text()).toBe('<lookup><url>http://backend.internal:8080/ws/restInvoices</url></lookup>');
    } finally {
      target.close();
    }
  });

  test('a backend named by a directory gets a proxy service, and the directory rule then sends the application there', async ({
    page,
    request,
  }) => {
    test.setTimeout(30000);
    // Two real backends: a directory that answers, for each name, the address of a service on the second one, which
    // the application then calls directly. Both send Content-Length, so that Mimicway captures their answers.
    const listen = (handler) =>
      new Promise((resolve) => {
        const server = http.createServer(handler);
        server.listen(0, '127.0.0.1', () => resolve(server));
      });
    const send = (res, type, body) => {
      res.writeHead(200, { 'content-type': type, 'content-length': Buffer.byteLength(body) });
      res.end(body);
    };
    const backend = await listen((req, res) => send(res, 'application/json', `{"served":"${req.url}"}`));
    const backendOrigin = `http://localhost:${backend.address().port}`;
    const directory = await listen((req, res) =>
      send(res, 'text/xml', `<lookup><url>${backendOrigin}/ws/${req.url.split('/').pop()}</url></lookup>`),
    );

    try {
      const created = await request.post(`${API}/services`, {
        data: proxyService('directory', directory.address().port),
      });
      expect(created.ok()).toBe(true);
      await request.post(`${API}/services/directory/observe`);
      for (const name of ['restOrders', 'restStock', 'restBilling']) {
        const r = await request.get(`http://localhost:7342/directory/service/9f86d081884c7d65/${name}`);
        expect(r.status()).toBe(200);
      }

      await page.goto('/');
      await page.waitForLoadState('networkidle');
      await page.getByText('No group').click();
      await page.getByTestId('service-card-configure-button-directory').click();
      await page.getByTestId('observation-refresh-suggestions-button-directory').click();

      const listed = page.getByTestId('observation-linked-backend-directory-0');
      await expect(listed).toContainText(backendOrigin);
      await expect(listed).toContainText(`named by GET /service/{hash}/{name}, such as ${backendOrigin}/ws/`);
      await docsScreenshot(
        page,
        'observation-linked-backends.png',
        '[data-testid="observation-linked-backends-directory"]',
      );

      // The service form opens filled; saving it goes through the usual checks, then back to the directory.
      await page.getByTestId('observation-create-service-directory-0').click();
      await expect(page.getByTestId('service-form-name-input')).toHaveValue('localhost');
      await expect(page.getByTestId('service-form-target-input')).toHaveValue(backendOrigin);
      await page.getByTestId('service-form-submit-button').click();
      await expect(page.getByTestId('observation-panel-directory')).toBeVisible();

      await page.getByTestId('observation-refresh-suggestions-button-directory').click();
      await expect(page.getByTestId('observation-linked-backend-directory-0')).toContainText(
        'Served by the service localhost.',
      );
      await expect(page.getByTestId('observation-create-service-directory-0')).toHaveCount(0);
      const card = page.getByTestId('observation-suggestion-directory-0-0');
      await expect(card).toContainText(
        '<url>{{header.x-forwarded-proto | default("http")}}://{{header.host}}/localhost/ws/{{path.name}}</url>',
      );
      await expect(page.getByTestId('observation-suggestion-redirect-directory-0-0')).toHaveText(
        'Sends the application to the service localhost in Mimicway, instead of the backend behind it.',
      );

      await page.getByTestId('observation-use-suggestion-directory-0-0').click();
      await page.getByTestId('rule-form-name-input').fill('any-service');
      await page.getByTestId('rule-form-submit-button').click();
      await expect(page.getByTestId('rule-list-item-any-service')).toBeVisible();

      // Once the directory is mocked, the application it serves is sent to Mimicway, which relays to the backend.
      const saved = await (await request.get(`${API}/services/directory`)).json();
      const mocked = await request.put(`${API}/services/directory`, {
        data: { ...saved, is_mocked: true, real_target_url: '' },
      });
      expect(mocked.ok()).toBe(true);
      const answer = await request.get('http://localhost:7342/directory/service/0123456789abcdef/restInvoices');
      expect(await answer.text()).toBe('<lookup><url>http://localhost:7342/localhost/ws/restInvoices</url></lookup>');
      const followed = await request.get('http://localhost:7342/localhost/ws/restInvoices');
      expect(await followed.json()).toEqual({ served: '/ws/restInvoices' });
    } finally {
      backend.close();
      directory.close();
    }
  });
});
