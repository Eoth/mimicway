import { test, expect } from '@playwright/test';

const API = 'http://localhost:7342/api';

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

function validRule(name, overrides = {}) {
  return {
    name,
    method: 'GET',
    sub_path: null,
    action: 'mock',
    pre_script: null,
    script: null,
    post_script: null,
    conditions: { all_of: [], any_of: [] },
    response: {
      status: 200,
      headers: [{ name: 'Content-Type', value: 'application/json' }],
      body: [{ type: 'Template', template: '{"ok":true}' }],
      chaos: null,
    },
    ...overrides,
  };
}

test.describe('Service CRUD', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('create, read, update, delete a service', async ({ request }) => {
    const svc = validService('test-svc', { listen_path: '/v1/*' });
    const create = await request.post(`${API}/services`, { data: svc });
    expect(create.status()).toBe(201);

    const get = await request.get(`${API}/services/test-svc`);
    expect(get.status()).toBe(200);
    const body = await get.json();
    expect(body.name).toBe('test-svc');

    svc.real_target_url = 'http://new-backend:9090';
    const update = await request.put(`${API}/services/test-svc`, { data: svc });
    expect(update.status()).toBe(200);

    const del = await request.delete(`${API}/services/test-svc`);
    expect(del.status()).toBe(204);

    const get2 = await request.get(`${API}/services/test-svc`);
    expect(get2.status()).toBe(404);
  });

  test('create service with rule and verify mock response', async ({ request }) => {
    const svc = validService('mock-svc', {
      listen_path: '/hello',
      rules: [
        validRule('hello-rule', {
          response: {
            status: 200,
            headers: [{ name: 'Content-Type', value: 'application/json' }],
            body: [{ type: 'Template', template: '{"message":"hello"}' }],
            chaos: null,
          },
        }),
      ],
    });
    await request.post(`${API}/services`, { data: svc });

    const mock = await request.get('http://localhost:7342/mock-svc/hello');
    expect(mock.status()).toBe(200);
    const body = await mock.json();
    expect(body.message).toBe('hello');
  });

  test('same service name in different groups is allowed', async ({ request }) => {
    await request.post(`${API}/groups`, { data: { name: 'grp-x', code: '', admins: [], members: [] } });
    await request.post(`${API}/groups`, { data: { name: 'grp-y', code: '', admins: [], members: [] } });

    const r1 = await request.post(`${API}/services`, { data: validService('shared-name', { group_name: 'grp-x' }) });
    expect(r1.status()).toBe(201);

    const r2 = await request.post(`${API}/services`, { data: validService('shared-name', { group_name: 'grp-y' }) });
    expect(r2.status()).toBe(201);
  });

  test('duplicate service name in same group returns 409', async ({ request }) => {
    await request.post(`${API}/services`, { data: validService('dup-svc') });
    const dup = await request.post(`${API}/services`, { data: validService('dup-svc') });
    expect(dup.status()).toBe(409);
  });

  test('empty listen_path creates catch-all', async ({ request }) => {
    const svc = validService('catchall', {
      listen_path: '',
      rules: [validRule('catch')],
    });
    await request.post(`${API}/services`, { data: svc });

    const resp = await request.get('http://localhost:7342/catchall/anything/here');
    expect(resp.status()).toBe(200);
  });
});

test.describe('Groups', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('create group, assign service, verify URL prefix', async ({ request }) => {
    const grp = await request.post(`${API}/groups`, {
      data: { name: 'team-test', code: '', admins: [], members: [] },
    });
    expect(grp.status()).toBe(201);
    const group = await grp.json();
    expect(group.code.length).toBe(5);

    const svc = validService('grp-svc', {
      group_name: 'team-test',
      rules: [validRule('grp-rule')],
    });
    await request.post(`${API}/services`, { data: svc });

    const prefixed = await request.get(`http://localhost:7342/${group.code}/grp-svc/anything`);
    expect(prefixed.status()).toBe(200);

    const unprefixed = await request.get('http://localhost:7342/grp-svc/anything');
    expect(unprefixed.status()).not.toBe(200);
  });

  test('delete group dissociates services', async ({ request }) => {
    await request.post(`${API}/groups`, {
      data: { name: 'del-grp', code: '', admins: [], members: [] },
    });
    await request.post(`${API}/services`, {
      data: validService('orphan-svc', { group_name: 'del-grp' }),
    });

    const del = await request.delete(`${API}/groups/del-grp`);
    expect(del.status()).toBe(204);

    const svc = await request.get(`${API}/services/orphan-svc`);
    const body = await svc.json();
    expect(body.group_name).toBeNull();
  });

  // Only a code typed by hand can collide: a generated code skips the codes in use (generate_code,
  // src/server/codegen.rs).
  test('group code uniqueness', async ({ request }) => {
    await request.post(`${API}/groups`, {
      data: { name: 'grp1', code: 'abc12', admins: [], members: [] },
    });
    const dup = await request.post(`${API}/groups`, {
      data: { name: 'grp2', code: 'abc12', admins: [], members: [] },
    });
    expect(dup.status()).toBe(409);
  });
});

// A service is identified by its group and its name: two groups may each hold a service of the same name. The deletion
// checks are scenarios (services.scenarios.json); this one is not, because the URL it expects holds the code of the
// group, known only once the group exists.
test.describe('Service identity across groups', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  async function expandGroup(page, groupName) {
    const header = page.locator('.group-header', { hasText: groupName });
    if ((await header.getAttribute('aria-expanded')) === 'false') {
      await header.click();
    }
  }

  test('the test URL of a grouped service is the same in the list and in its form', async ({ page, request }) => {
    const grp = await (
      await request.post(`${API}/groups`, {
        data: { name: 'url-parity-grp', code: '', admins: [], members: [] },
      })
    ).json();
    await request.post(`${API}/services`, {
      data: validService('url-parity-svc', { listen_path: '/v1/*', group_name: 'url-parity-grp' }),
    });

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expandGroup(page, 'url-parity-grp');

    const card = page.locator('.service-card', { hasText: 'url-parity-svc' });
    const listUrl = await card.locator('.detail-row', { hasText: 'Test URL' }).locator('code').textContent();
    expect(listUrl).toBe(`/${grp.code}/url-parity-svc/v1/*`);

    await card.getByRole('button', { name: 'Configure the service url-parity-svc' }).click();
    await page.getByRole('button', { name: 'Edit the service' }).click();

    const editUrl = page.locator('.url-preview code');
    await expect(editUrl).toContainText(`/${grp.code}/url-parity-svc/v1/*`);
  });
});

test.describe('Import/Export', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('import config with groups generates codes', async ({ request }) => {
    const config = {
      services: [validService('imp-svc', { group_name: 'imp-grp' })],
      groups: [{ name: 'imp-grp', code: '', admins: [], members: [] }],
    };
    const put = await request.put(`${API}/config`, { data: config });
    expect(put.status()).toBe(200);

    const groups = await (await request.get(`${API}/groups`)).json();
    expect(groups.length).toBe(1);
    expect(groups[0].code.length).toBe(5);
    expect(groups[0].code).not.toBe('');
  });
});

test.describe('Rules with method and sub_path', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('rule method filters requests', async ({ request }) => {
    const svc = validService('method-svc', {
      rules: [
        validRule('get-only', { method: 'GET' }),
        validRule('post-only', {
          method: 'POST',
          response: {
            status: 201,
            headers: [],
            body: [{ type: 'Template', template: '{"created":true}' }],
            chaos: null,
          },
        }),
      ],
    });
    await request.post(`${API}/services`, { data: svc });

    const get = await request.get('http://localhost:7342/method-svc/test');
    expect(get.status()).toBe(200);

    const post = await request.post('http://localhost:7342/method-svc/test');
    expect(post.status()).toBe(201);
  });

  test('sub_path narrows matching', async ({ request }) => {
    const svc = validService('subpath-svc', {
      rules: [
        validRule('users', {
          sub_path: '/users/{id}',
          response: {
            status: 200,
            headers: [],
            body: [{ type: 'Template', template: '{"user":"{{path.id}}"}' }],
            chaos: null,
          },
        }),
      ],
    });
    await request.post(`${API}/services`, { data: svc });

    const ok = await request.get('http://localhost:7342/subpath-svc/users/42');
    expect(ok.status()).toBe(200);
    expect((await ok.json()).user).toBe('42');

    const miss = await request.get('http://localhost:7342/subpath-svc/orders/1');
    expect(miss.status()).toBe(404);
  });

  test('listen_path + sub_path combined', async ({ request }) => {
    const svc = validService('combo-svc', {
      listen_path: '/api/v4',
      rules: [
        validRule('get-user', {
          sub_path: '/user',
          response: {
            status: 200,
            headers: [],
            body: [{ type: 'Template', template: '{"endpoint":"user"}' }],
            chaos: null,
          },
        }),
        validRule('get-projects', {
          sub_path: '/projects/{id}',
          response: {
            status: 200,
            headers: [],
            body: [{ type: 'Template', template: '{"project":"{{path.id}}"}' }],
            chaos: null,
          },
        }),
      ],
    });
    await request.post(`${API}/services`, { data: svc });

    const user = await request.get('http://localhost:7342/combo-svc/api/v4/user');
    expect(user.status()).toBe(200);
    expect((await user.json()).endpoint).toBe('user');

    const proj = await request.get('http://localhost:7342/combo-svc/api/v4/projects/42');
    expect(proj.status()).toBe(200);
    expect((await proj.json()).project).toBe('42');

    const miss = await request.get('http://localhost:7342/combo-svc/api/v4/unknown');
    expect(miss.status()).toBe(404);
  });
});

test.describe('Script execution', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('script result available in template', async ({ request }) => {
    const svc = validService('script-svc', {
      rules: [
        validRule('scripted', {
          script: '#{ greeting: "hello" }',
          response: {
            status: 200,
            headers: [],
            body: [{ type: 'Template', template: '{"msg":"{{script.greeting}}"}' }],
            chaos: null,
          },
        }),
      ],
    });
    await request.post(`${API}/services`, { data: svc });

    const resp = await request.get('http://localhost:7342/script-svc/test');
    expect(resp.status()).toBe(200);
    expect((await resp.json()).msg).toBe('hello');
  });

  test('script validation endpoint', async ({ request }) => {
    const valid = await request.post(`${API}/script/validate`, {
      data: { script: 'let x = 42; x' },
    });
    expect((await valid.json()).valid).toBe(true);

    const invalid = await request.post(`${API}/script/validate`, {
      data: { script: 'let x =' },
    });
    expect((await invalid.json()).valid).toBe(false);
  });
});

test.describe('WSDL bypass', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('wsdl_mode mock applies rules even for ?wsdl', async ({ request }) => {
    const svc = validService('wsdl-svc', {
      wsdl_mode: 'mock',
      rules: [validRule('wsdl-rule')],
    });
    await request.post(`${API}/services`, { data: svc });

    const resp = await request.get('http://localhost:7342/wsdl-svc/test?wsdl');
    expect(resp.status()).toBe(200);
  });
});

test.describe('Health endpoint', () => {
  test('GET /api/health returns 200', async ({ request }) => {
    const resp = await request.get(`${API}/health`);
    expect(resp.status()).toBe(200);
  });
});
