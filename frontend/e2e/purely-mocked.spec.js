import { test, expect } from '@playwright/test';

// A purely mocked service (empty real_target_url) has nothing to forward to: a request no rule matches gets a 404 that
// says why, where a mocked service that has a target answers a plain 404.
const BASE = 'http://localhost:7342';
const API = `${BASE}/api`;

function validService(name, overrides = {}) {
  return {
    name,
    listen_path: '',
    real_target_url: 'http://backend:8080',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
    ...overrides,
  };
}

test.describe('Purely mocked service: what it answers', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('a request no rule matches gets a 404 that names the service purely mocked', async ({ request }) => {
    await request.post(`${API}/services`, {
      data: validService('nocible-nomatch', { real_target_url: '' }),
    });

    const resp = await request.get(`${BASE}/nocible-nomatch/anything`);
    expect(resp.status()).toBe(404);
    const body = await resp.text();
    expect(body).toContain('purely mocked');
  });

  test('on a service with a target, the 404 of an unmatched request is a plain one', async ({ request }) => {
    await request.post(`${API}/services`, {
      data: validService('avecible-nomatch'),
    });

    const resp = await request.get(`${BASE}/avecible-nomatch/anything`);
    expect(resp.status()).toBe(404);
    const body = await resp.text();
    expect(body).not.toContain('purely mocked');
  });

  test('a purely mocked service still answers with the rule that matches', async ({ request }) => {
    await request.post(`${API}/services`, {
      data: validService('nocible-avecregle', {
        real_target_url: '',
        rules: [
          {
            name: 'ok-rule',
            method: 'GET',
            sub_path: null,
            action: 'mock',
            pre_script: null,
            script: null,
            post_script: null,
            conditions: { all_of: [], any_of: [] },
            response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'ok' }], chaos: null },
          },
        ],
      }),
    });

    const resp = await request.get(`${BASE}/nocible-avecregle/anything`);
    expect(resp.status()).toBe(200);
    expect(await resp.text()).toBe('ok');
  });
});
