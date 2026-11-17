import { test, expect } from './fixtures.mjs';
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
      // The name is escaped for XML by the pipes that follow it.
      await expect(card).toContainText('<url>http://backend.internal:8080/ws/{{path.name | replace("&", "&amp;")');
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
        '<url>{{header.x-forwarded-proto | default("http")}}://{{header.host}}/localhost/ws/{{path.name |',
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

  test('each operation of a SOAP endpoint gets its suggestion, a rule may accept several values, and an unexplained error is set aside', async ({
    page,
    request,
  }) => {
    test.setTimeout(30000);
    // One SOAP address, two operations. GetStatus answers by reference, R1 and R2 the same way, and once fails with a
    // server error for a reference that otherwise succeeds.
    let r1Calls = 0;
    const target = await new Promise((resolve) => {
      const server = http.createServer((req, res) => {
        let request = '';
        req.on('data', (chunk) => (request += chunk));
        req.on('end', () => {
          let status = 200;
          let inner = '<Pong/>';
          const ref = /<o:ref>(.*?)<\/o:ref>/.exec(request)?.[1];
          if (ref) {
            if (ref === 'R1') r1Calls += 1;
            if (ref === 'R1' && r1Calls === 3) status = 500;
            inner = `<GetStatusResponse><status>${ref === 'R3' ? 'CLOSED' : 'OPEN'}</status></GetStatusResponse>`;
          }
          const body =
            status === 500
              ? '<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body><soap:Fault/></soap:Body></soap:Envelope>'
              : `<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body>${inner}</soap:Body></soap:Envelope>`;
          res.writeHead(status, { 'content-type': 'text/xml', 'content-length': Buffer.byteLength(body) });
          res.end(body);
        });
      });
      server.listen(0, '127.0.0.1', () => resolve(server));
    });
    const targetPort = target.address().port;
    const envelope = (operation, inner) =>
      '<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/" xmlns:o="urn:orders"><soap:Body>' +
      `<o:${operation}>${inner}</o:${operation}></soap:Body></soap:Envelope>`;

    try {
      const created = await request.post(`${API}/services`, { data: proxyService('orders-soap', targetPort) });
      expect(created.ok()).toBe(true);
      await request.post(`${API}/services/orders-soap/observe`);
      const calls = [
        ['Ping', ''],
        ['Ping', ''],
        ['Ping', ''],
        ...['R1', 'R1', 'R2', 'R3', 'R3', 'R1'].map((ref) => ['GetStatus', `<o:ref>${ref}</o:ref>`]),
      ];
      for (const [operation, inner] of calls) {
        await request.post('http://localhost:7342/orders-soap/ws/OrderService', {
          data: envelope(operation, inner),
          headers: { 'content-type': 'text/xml' },
        });
      }

      await page.goto('/');
      await page.waitForLoadState('networkidle');
      await page.getByText('No group').click();
      await page.getByTestId('service-card-configure-button-orders-soap').click();
      await page.getByTestId('observation-refresh-suggestions-button-orders-soap').click();

      await expect(page.getByTestId('observation-suggestion-operation-orders-soap-0')).toHaveText(
        'SOAP operation GetStatus: its rules answer only the calls that name it.',
      );
      await expect(page.getByTestId('observation-suggestion-operation-orders-soap-1')).toHaveText(
        'SOAP operation Ping: its rules answer only the calls that name it.',
      );
      await expect(page.getByTestId('observation-suggestion-left-out-orders-soap-0')).toHaveText(
        '1 call was left out: it got a server error (500) that nothing in the request explains.',
      );
      const both = page.getByTestId('observation-suggestion-orders-soap-0-0');
      await expect(both).toContainText('if XML element of the body "Envelope/Body/GetStatus/ref" is "R1" or "R2"');

      await page.getByTestId('observation-use-suggestion-orders-soap-0-0').click();
      await page.getByTestId('rule-form-name-input').fill('open-status');
      await page.getByTestId('rule-form-submit-button').click();
      await expect(page.getByTestId('rule-list-item-open-status')).toBeVisible();

      // Mocked, with no backend left: the rule answers R2 and nothing else.
      const saved = await (await request.get(`${API}/services/orders-soap`)).json();
      const rule = saved.rules.find((r) => r.name === 'open-status');
      expect(rule.conditions.all_of).toEqual([
        { source: { type: 'XPath', key: 'Envelope/Body/GetStatus' }, operator: { type: 'Exists' } },
      ]);
      expect(rule.conditions.any_of.map((c) => c.operator.value)).toEqual(['R1', 'R2']);
      const updated = await request.put(`${API}/services/orders-soap`, {
        data: { ...saved, is_mocked: true, real_target_url: '' },
      });
      expect(updated.ok()).toBe(true);
      const answer = await request.post('http://localhost:7342/orders-soap/ws/OrderService', {
        data: envelope('GetStatus', '<o:ref>R2</o:ref>'),
      });
      expect(answer.status()).toBe(200);
      expect(await answer.text()).toContain('<status>OPEN</status>');
      const other = await request.post('http://localhost:7342/orders-soap/ws/OrderService', {
        data: envelope('GetStatus', '<o:ref>R3</o:ref>'),
      });
      expect(await other.text()).not.toContain('<status>OPEN</status>');
    } finally {
      target.close();
    }
  });
  test('an operation that repeats its reference, and one that answers a block per item, answer requests never seen', async ({
    page,
    request,
  }) => {
    test.setTimeout(40000);
    // GetStatus repeats the reference it is asked about; Book answers one block per slot, with its date and type.
    const escape = (text) => text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
    const answerOf = (request) => {
      const ref = /<o:ref>(.*?)<\/o:ref>/.exec(request)?.[1];
      if (request.includes('<o:GetStatus>')) {
        return `<GetStatusResponse><ref>${ref}</ref><state>OPEN</state></GetStatusResponse>`;
      }
      const slots = [...request.matchAll(/<o:slot><o:date>(.*?)<\/o:date><o:type>(.*?)<\/o:type><\/o:slot>/g)];
      const blocks = slots.map(([, date, type]) => `<booked><date>${date}</date><type>${type}</type></booked>`);
      return `<BookResponse>${blocks.join('')}</BookResponse>`;
    };
    const target = await new Promise((resolve) => {
      const server = http.createServer((req, res) => {
        let body = '';
        req.on('data', (chunk) => (body += chunk));
        req.on('end', () => {
          const answer = `<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body>${answerOf(body)}</soap:Body></soap:Envelope>`;
          res.writeHead(200, { 'content-type': 'text/xml', 'content-length': Buffer.byteLength(answer) });
          res.end(answer);
        });
      });
      server.listen(0, '127.0.0.1', () => resolve(server));
    });
    const targetPort = target.address().port;
    const envelope = (operation, inner) =>
      '<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/" xmlns:o="urn:orders"><soap:Body>' +
      `<o:${operation}>${inner}</o:${operation}></soap:Body></soap:Envelope>`;
    const status = (ref) => envelope('GetStatus', `<o:ref>${escape(ref)}</o:ref>`);
    const book = (slots) =>
      envelope(
        'Book',
        slots
          .map(([date, type]) => `<o:slot><o:date>${date}</o:date><o:type>${escape(type)}</o:type></o:slot>`)
          .join(''),
      );
    const send = (port, body) =>
      request.post(`http://${port === 7342 ? 'localhost:7342/stock-soap' : `127.0.0.1:${port}`}/ws/StockService`, {
        data: body,
        headers: { 'content-type': 'text/xml' },
      });

    try {
      const created = await request.post(`${API}/services`, { data: proxyService('stock-soap', targetPort) });
      expect(created.ok()).toBe(true);
      await request.post(`${API}/services/stock-soap/observe`);
      for (const body of [
        status('R-100'),
        status('R-200'),
        status('R-300'),
        book([
          ['2026-03-01', 'A'],
          ['2026-03-02', 'B'],
        ]),
        book([['2026-04-09', 'C']]),
        book([
          ['2026-05-01', 'A'],
          ['2026-05-02', 'A'],
          ['2026-05-03', 'B'],
        ]),
      ]) {
        expect((await send(7342, body)).ok()).toBe(true);
      }

      // Requests never seen, and what the real backend answers to them.
      const unseen = [
        status('R-999 & co'),
        book([
          ['2027-01-01', 'X'],
          ['2027-01-02', 'Y<'],
        ]),
        book([['2027-02-01', 'Z']]),
        book([
          ['2027-03-01', 'P'],
          ['2027-03-02', 'Q'],
          ['2027-03-03', 'R'],
        ]),
      ];
      const expected = [];
      for (const body of unseen) expected.push(await (await send(targetPort, body)).text());

      await page.goto('/');
      await page.waitForLoadState('networkidle');
      await page.getByText('No group').click();
      await page.getByTestId('service-card-configure-button-stock-soap').click();
      await page.getByTestId('observation-refresh-suggestions-button-stock-soap').click();
      await expect(page.getByTestId('observation-suggestion-stock-soap-0-0')).toContainText(
        'no condition: the response writes one block per "slot" of the request, with its "date" and "type" (3 calls)',
      );
      await expect(page.getByTestId('observation-suggestion-stock-soap-1-0')).toContainText(
        'no condition: the response repeats XML element of the body "ref" (3 calls)',
      );

      // The list's rule through the form, which receives its script; the reference's rule as suggested.
      await page.getByTestId('observation-use-suggestion-stock-soap-0-0').click();
      await page.getByTestId('rule-form-name-input').fill('book');
      await page.getByTestId('rule-form-submit-button').click();
      await expect(page.getByTestId('rule-list-item-book')).toBeVisible();
      const suggestions = await (await request.get(`${API}/services/stock-soap/suggestions`)).json();
      const reference = suggestions.find((s) => s.operation === 'GetStatus').rule;
      const saved = await (await request.get(`${API}/services/stock-soap`)).json();
      expect(saved.rules.find((r) => r.name === 'book').script).toContain('parse_xml_items');
      const statusRule = {
        name: 'status',
        method: reference.method,
        sub_path: reference.sub_path,
        action: 'mock',
        script: reference.script,
        conditions: reference.conditions,
        response: reference.response,
      };

      // The backend gone, the mock alone answers each request never seen as the backend did.
      target.close();
      const updated = await request.put(`${API}/services/stock-soap`, {
        data: { ...saved, rules: [...saved.rules, statusRule], is_mocked: true, real_target_url: '' },
      });
      expect(updated.ok()).toBe(true);
      for (const [i, body] of unseen.entries()) {
        const answer = await send(7342, body);
        expect(answer.status()).toBe(200);
        expect(await answer.text()).toBe(expected[i]);
      }
    } finally {
      target.close();
    }
  });
});
