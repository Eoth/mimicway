import { test, expect } from '@playwright/test';

const API = 'http://localhost:7342/api';

function validService(name, overrides = {}) {
  return {
    name,
    listen_path: '/v1/*',
    real_target_url: 'http://backend:8080',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
    ...overrides,
  };
}

test.describe('Security: route protection', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('API accepts service with empty listen_path (catch-all)', async ({ request }) => {
    const res = await request.post(`${API}/services`, {
      data: validService('catchall-svc', { listen_path: '' }),
    });
    expect(res.status()).toBe(201);
  });

  test('API accepts service with listen_path "/*"', async ({ request }) => {
    const res = await request.post(`${API}/services`, {
      data: validService('wildcard-svc', { listen_path: '/*' }),
    });
    expect(res.status()).toBe(201);
  });

  test('API rejects service named "api"', async ({ request }) => {
    const res = await request.post(`${API}/services`, {
      data: validService('api'),
    });
    expect(res.status()).toBe(400);
  });

  test('internal API routes remain accessible with services registered', async ({ request }) => {
    await request.post(`${API}/services`, {
      data: validService('my-svc'),
    });
    const configRes = await request.get(`${API}/config`);
    expect(configRes.ok()).toBe(true);
    const servicesRes = await request.get(`${API}/services`);
    expect(servicesRes.ok()).toBe(true);
    const logsRes = await request.get(`${API}/logs`);
    expect(logsRes.ok()).toBe(true);
  });

  test('reset endpoint removes all services', async ({ request }) => {
    await request.post(`${API}/services`, {
      data: validService('svc1'),
    });
    const before = await request.get(`${API}/services`);
    const beforeData = await before.json();
    expect(beforeData.length).toBeGreaterThan(0);

    const resetRes = await request.delete(`${API}/config/reset`);
    expect(resetRes.status()).toBe(204);

    const after = await request.get(`${API}/services`);
    const afterData = await after.json();
    expect(afterData.length).toBe(0);
  });

  test('put_config rejects config with reserved name', async ({ request }) => {
    const res = await request.put(`${API}/config`, {
      data: {
        services: [validService('api')],
        groups: [],
      },
    });
    expect(res.status()).toBe(400);
  });
});

test.describe('Uniqueness: service_key collision', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('creating a service with a new name succeeds (201)', async ({ request }) => {
    const res = await request.post(`${API}/services`, {
      data: validService('unique-svc'),
    });
    expect(res.status()).toBe(201);
  });

  test('creating a second service with the same name is rejected (409)', async ({ request }) => {
    const first = await request.post(`${API}/services`, {
      data: validService('dup-svc'),
    });
    expect(first.status()).toBe(201);

    const second = await request.post(`${API}/services`, {
      data: validService('dup-svc'),
    });
    expect(second.status()).toBe(409);
    const body = await second.json();
    expect(body.error).toContain('already exists');
  });

  test('updating an existing service succeeds (PUT)', async ({ request }) => {
    await request.post(`${API}/services`, {
      data: validService('edit-svc'),
    });
    const res = await request.put(`${API}/services/edit-svc`, {
      data: validService('edit-svc', { listen_path: '/v2/*' }),
    });
    expect(res.ok()).toBe(true);
  });

  test('updating a non-existent service returns 404', async ({ request }) => {
    const res = await request.put(`${API}/services/ghost`, {
      data: validService('ghost'),
    });
    expect(res.status()).toBe(404);
  });

  test('a second service with an existing name leaves the first one unchanged', async ({ request }) => {
    await request.post(`${API}/services`, {
      data: validService('keep-me', { listen_path: '/v1/original/*' }),
    });

    const second = await request.post(`${API}/services`, {
      data: validService('keep-me', { listen_path: '/v1/other/*' }),
    });
    expect(second.status()).toBe(409);
    const check = await request.get(`${API}/services/keep-me`);
    const svc = await check.json();
    expect(svc.listen_path).toBe('/v1/original/*');
  });
});

test.describe('Rule name uniqueness', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  function rule(name) {
    return {
      name,
      method: 'GET',
      sub_path: null,
      action: 'mock',
      pre_script: null,
      script: null,
      post_script: null,
      conditions: { all_of: [], any_of: [] },
      response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'ok' }], chaos: null },
    };
  }

  test('rejects duplicate rule names in the same service', async ({ request }) => {
    const svc = validService('rule-test', { rules: [rule('dup'), rule('dup')] });
    const res = await request.post(`${API}/services`, { data: svc });
    expect(res.status()).toBe(400);
    const body = await res.json();
    expect(body.error).toContain('used several times');
  });

  test('accepts same rule name across different services', async ({ request }) => {
    const svc1 = validService('svc-a', { rules: [rule('shared')] });
    const svc2 = validService('svc-b', { rules: [rule('shared')] });
    const r1 = await request.post(`${API}/services`, { data: svc1 });
    expect(r1.status()).toBe(201);
    const r2 = await request.post(`${API}/services`, { data: svc2 });
    expect(r2.status()).toBe(201);
  });
});

test.describe('Security: browser policy of the UI', () => {
  test('the UI runs under its content security policy without any violation', async ({ page, request }) => {
    await request.delete(`${API}/config/reset`);
    const violations = [];
    page.on('console', (msg) => {
      if (/content security policy/i.test(msg.text())) violations.push(msg.text());
    });
    await page.addInitScript(() => {
      document.addEventListener('securitypolicyviolation', (e) => {
        console.error(`Content Security Policy violation: ${e.violatedDirective} ${e.blockedURI}`);
      });
    });
    const response = await page.goto('/');
    expect(response.headers()['content-security-policy']).toContain("frame-ancestors 'none'");
    await page.locator('[data-testid="app-load-demo-button"]').click();
    await page.locator('[data-testid="app-nav-logs-button"]').click();
    await page.locator('[data-testid="app-theme-toggle-button"]').click();
    await page.locator('[data-testid="app-title-button"]').click();
    await page.waitForLoadState('networkidle');
    expect(violations).toEqual([]);
  });
});
