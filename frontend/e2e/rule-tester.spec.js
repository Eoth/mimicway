import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const API = 'http://localhost:7342/api';
const BASE = 'http://localhost:7342';

// The rule tester replays the rule being edited, read only, against a request of the log, and says why it would or
// would not match it; the condition form helps with path and query parameters. Each test sends a real request first,
// so that the log holds one.

function validService(name, overrides = {}) {
  return {
    name,
    listen_path: '/*',
    real_target_url: 'http://backend:8080',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
    ...overrides,
  };
}

async function openService(page, serviceName) {
  await page.goto('/');
  await page.waitForLoadState('networkidle');
  const group = page.locator('button[aria-expanded]').first();
  if ((await group.count()) > 0 && (await group.getAttribute('aria-expanded')) === 'false') {
    await group.click();
  }
  await page.getByRole('button', { name: new RegExp(`Configure the service ${serviceName}`) }).click();
}

async function openAddRuleForm(page, serviceName) {
  await openService(page, serviceName);
  await page.getByRole('button', { name: /Add a rule/ }).click();
}

test.describe('Rule tester: conditions against a real request', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('explains a path parameter entered as a query parameter', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('tester-svc', { listen_path: '/{id}/*' }),
    });

    // No rule yet: the request gets a 404, and the log keeps it all the same.
    const captured = await request.get(`${BASE}/tester-svc/42/details`);
    expect(captured.status()).toBe(404);

    await openAddRuleForm(page, 'tester-svc');
    await page.locator('input#rule-name').fill('mauvais-choix');

    await page.getByRole('button', { name: '+ AND condition' }).click();
    await page.locator('#cond-source').selectOption('QueryParam');
    await page.locator('#cond-key').fill('id');
    await page.locator('#cond-val').fill('42');
    await page.getByRole('button', { name: 'OK' }).click();

    // A configuration reset leaves the request log alone, and the log lists the newest request first (recent(),
    // src/server/request_log.rs): the first entry of this path is this run's, whatever earlier runs left.
    const logSelect = page.locator('#rule-tester-log');
    await expect(logSelect).toBeVisible();
    const matchingOption = logSelect.locator('option', { hasText: 'tester-svc/42/details' }).first();
    await expect(matchingOption).toBeAttached();
    const optionValue = await matchingOption.getAttribute('value');
    await logSelect.selectOption(optionValue);

    await page.getByRole('button', { name: /Test against this request/ }).click();

    await expect(page.getByText(/would not match this request/)).toBeVisible();
    await expect(page.getByText(/value found: none/)).toBeVisible();
    await expect(page.getByText(/present as a path parameter/)).toBeVisible();
    // The hint is worded by the server in the language of the request: test again after each language switch.
    await docsScreenshot(page, 'rule-tester-hint.png', '[data-testid="rule-tester-result"]', {
      afterSwitch: async (p) => {
        const answered = p.waitForResponse('**/api/rule-test');
        await p.getByTestId('rule-tester-test-button').click();
        await answered;
      },
    });
  });

  test('a path parameter is picked from those of the listen path, not typed', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('strict-svc', { listen_path: '/{orderId}/*' }),
    });

    await openAddRuleForm(page, 'strict-svc');
    await page.getByRole('button', { name: '+ AND condition' }).click();
    await page.locator('#cond-source').selectOption('PathParam');

    const keyField = page.locator('#cond-key');
    await expect(keyField).toHaveJSProperty('tagName', 'SELECT');
    const optionValues = await keyField.locator('option').evaluateAll((opts) => opts.map((o) => o.value));
    expect(optionValues.filter(Boolean)).toEqual(['orderId']);
  });

  test('a service with a static listen path offers no path parameter source', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('static-svc', { listen_path: '/fixed/path' }),
    });

    await openAddRuleForm(page, 'static-svc');
    await page.getByRole('button', { name: '+ AND condition' }).click();

    const sourceSelect = page.locator('#cond-source');
    const optionValues = await sourceSelect.locator('option').evaluateAll((opts) => opts.map((o) => o.value));
    expect(optionValues).not.toContain('PathParam');
  });

  test('query parameter names seen in the traffic are suggested', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('autocomplete-qp-svc', { listen_path: '/*' }),
    });

    await request.get(`${BASE}/autocomplete-qp-svc/anything?customerRef=abc123`);

    await openAddRuleForm(page, 'autocomplete-qp-svc');
    await page.getByRole('button', { name: '+ AND condition' }).click();
    await page.locator('#cond-source').selectOption('QueryParam');

    const keyField = page.locator('#cond-key');
    const datalistId = await keyField.getAttribute('list');
    expect(datalistId).toBeTruthy();
    const suggestions = await page.locator(`#${datalistId} option`).evaluateAll((opts) => opts.map((o) => o.value));
    expect(suggestions).toContain('customerRef');

    // A suggestion, not a constraint: a name never seen can still be typed.
    await keyField.fill('brandNewParam');
    await expect(keyField).toHaveValue('brandNewParam');
  });

  test('warns when the logged body was truncated and a tested condition reads the body', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('truncation-svc', { listen_path: '/*' }),
    });

    // Larger than REQUEST_LOG_MAX_BODY_SIZE (16 KiB by default): the log keeps it truncated.
    const largeBody = 'x'.repeat(20000);
    await request.post(`${BASE}/truncation-svc/anything`, {
      data: largeBody,
      headers: { 'content-type': 'text/plain' },
    });

    await openAddRuleForm(page, 'truncation-svc');
    await page.locator('input#rule-name').fill('body-based-rule');

    // BodyRaw reads the whole body, so a truncated one can change its outcome.
    await page.getByRole('button', { name: '+ AND condition' }).click();
    await page.locator('#cond-source').selectOption('BodyRaw');
    await page.locator('#cond-op').selectOption('Exists');
    await page.getByRole('button', { name: 'OK' }).click();

    const logSelect = page.locator('#rule-tester-log');
    await expect(logSelect).toBeVisible();
    const matchingOption = logSelect.locator('option', { hasText: 'truncation-svc/anything' }).first();
    await expect(matchingOption).toBeAttached();
    const optionValue = await matchingOption.getAttribute('value');
    await logSelect.selectOption(optionValue);

    await page.getByRole('button', { name: /Test against this request/ }).click();

    await expect(page.getByText(/body of this request was truncated/)).toBeVisible();
  });

  test('no truncation warning when no tested condition reads the body', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('truncation-noop-svc', { listen_path: '/*' }),
    });

    const largeBody = 'x'.repeat(20000);
    await request.post(`${BASE}/truncation-noop-svc/anything?foo=bar`, {
      data: largeBody,
      headers: { 'content-type': 'text/plain' },
    });

    await openAddRuleForm(page, 'truncation-noop-svc');
    await page.locator('input#rule-name').fill('query-only-rule');
    // The logged request is a POST: the rule takes that method, so that the outcome (overall_matched) does not
    // turn on the method (method_matches).
    await page.locator('#rule-method').selectOption('POST');

    // A query parameter never depends on the body: no warning expected.
    await page.getByRole('button', { name: '+ AND condition' }).click();
    await page.locator('#cond-source').selectOption('QueryParam');
    await page.locator('#cond-key').fill('foo');
    await page.locator('#cond-val').fill('bar');
    await page.getByRole('button', { name: 'OK' }).click();

    const logSelect = page.locator('#rule-tester-log');
    await expect(logSelect).toBeVisible();
    const matchingOption = logSelect.locator('option', { hasText: 'truncation-noop-svc/anything' }).first();
    await expect(matchingOption).toBeAttached();
    const optionValue = await matchingOption.getAttribute('value');
    await logSelect.selectOption(optionValue);

    await page.getByRole('button', { name: /Test against this request/ }).click();

    await expect(page.getByText(/would match this request/)).toBeVisible();
    await expect(page.getByText(/body of this request was truncated/)).not.toBeVisible();
  });
});

// The tester runs the rule's scripts too (POST /api/rule-test): a script that fails at run time shows there, where the
// mock itself only logs the error and carries on.
test.describe('Rule tester: scripts', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('a script that fails at run time shows its error', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('script-error-svc', { listen_path: '/*' }),
    });
    await request.get(`${BASE}/script-error-svc/anything`);

    await openAddRuleForm(page, 'script-error-svc');
    await page.locator('input#rule-name').fill('script-casse');

    // A call to a function that does not exist: compiling the script (validate) accepts it, only running it fails.
    await page.getByRole('switch', { name: /Custom script/ }).click();
    await page.locator('#rule-script').fill('totally_undefined_fn(1, 2)');

    const logSelect = page.locator('#rule-tester-log');
    await expect(logSelect).toBeVisible();
    const matchingOption = logSelect.locator('option', { hasText: 'script-error-svc/anything' }).first();
    await expect(matchingOption).toBeAttached();
    await logSelect.selectOption(await matchingOption.getAttribute('value'));

    await page.getByRole('button', { name: /Test against this request/ }).click();

    await expect(page.getByText(/would match this request/)).toBeVisible();
    const errorBanner = page.getByTestId('rule-tester-script-errors');
    await expect(errorBanner).toBeVisible();
    await expect(errorBanner).toContainText('totally_undefined_fn');
    await expect(page.getByTestId('rule-tester-script-error-script')).toBeVisible();
    await docsScreenshot(page, 'rule-tester-script-error.png', '[data-testid="rule-tester-script-errors"]');
  });

  test('a correct lookup script shows no error', async ({ page, request }) => {
    // The path parameter comes from the listen path of the service, so the logged request has request.path.name
    // without any sub-path on the rule.
    await request.post(`${API}/services`, {
      data: validService('lookup-e2e-svc', { listen_path: '/lookup/{name}' }),
    });
    const captured = await request.get(`${BASE}/lookup-e2e-svc/lookup/billing`);
    expect(captured.status()).toBe(404); // no rule yet, logged all the same

    await openAddRuleForm(page, 'lookup-e2e-svc');
    await page.locator('input#rule-name').fill('lookup-service');

    // The script of the guide's example (rhai-scripts.md) and of the server's test
    // map_lookup_by_path_param_returns_correct_target_and_falls_back_for_unknown_key (src/server/intercept.rs).
    await page.getByRole('switch', { name: /Custom script/ }).click();
    await page
      .locator('#rule-script')
      .fill(
        'let mapping = #{ "billing": "svc-billing-042", "orders": "svc-orders-017" };\n' +
          'let name = request.path.name;\n' +
          'if mapping.contains(name) { #{ id: mapping[name], found: "true" } } else { #{ id: "unknown", found: "false" } }',
      );

    const logSelect = page.locator('#rule-tester-log');
    await expect(logSelect).toBeVisible();
    const matchingOption = logSelect.locator('option', { hasText: 'lookup-e2e-svc/lookup/billing' }).first();
    await expect(matchingOption).toBeAttached();
    await logSelect.selectOption(await matchingOption.getAttribute('value'));

    await page.getByRole('button', { name: /Test against this request/ }).click();

    await expect(page.getByText(/would match this request/)).toBeVisible();
    await expect(page.getByTestId('rule-tester-script-errors')).not.toBeVisible();
  });

  // A script can succeed and still return what its author did not expect: the tester shows what it returned, a nested
  // object as JSON (dynamic_field_to_string, src/engine/script.rs).
  test('shows what a successful script returned, a nested object as JSON', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('seeded-pick-object-svc', { listen_path: '/quote/{siret}' }),
    });
    const captured = await request.get(`${BASE}/seeded-pick-object-svc/quote/44306184100047`);
    expect(captured.status()).toBe(404); // no rule yet, logged all the same

    await openAddRuleForm(page, 'seeded-pick-object-svc');
    await page.locator('input#rule-name').fill('quote-rule');

    await page.getByRole('switch', { name: /Custom script/ }).click();
    await page
      .locator('#rule-script')
      .fill(
        'let villes = [\n' +
          '  #{ name: "Paris", cp: "75000", insee: "75056" },\n' +
          '  #{ name: "Lyon", cp: "69000", insee: "69123" }\n' +
          '];\n' +
          'let ville = seeded_pick(request.path.siret, villes);\n' +
          '#{ ville: ville, quoteId: "fixed-id" }',
      );

    const logSelect = page.locator('#rule-tester-log');
    await expect(logSelect).toBeVisible();
    const matchingOption = logSelect
      .locator('option', { hasText: 'seeded-pick-object-svc/quote/44306184100047' })
      .first();
    await expect(matchingOption).toBeAttached();
    await logSelect.selectOption(await matchingOption.getAttribute('value'));

    await page.getByRole('button', { name: /Test against this request/ }).click();

    await expect(page.getByText(/would match this request/)).toBeVisible();
    // No run-time error: the script succeeds, only its result is in question.
    await expect(page.getByTestId('rule-tester-script-errors')).not.toBeVisible();

    const resultPanel = page.getByTestId('rule-tester-script-results');
    await expect(resultPanel).toBeVisible();
    const slotPanel = page.getByTestId('rule-tester-script-result-script');
    await expect(slotPanel).toBeVisible();
    // A plain field shows its value.
    await expect(slotPanel).toContainText('{{script.quoteId}}');
    await expect(slotPanel).toContainText('fixed-id');
    // A nested field shows as valid JSON, never in Rhai's #{...} notation.
    await expect(slotPanel).toContainText('{{script.ville}}');
    await expect(slotPanel).not.toContainText('#{');
    const villeFieldText = await slotPanel.textContent();
    const jsonStart = villeFieldText.indexOf('{"');
    expect(jsonStart).toBeGreaterThan(-1);
    const villeJson = JSON.parse(villeFieldText.slice(jsonStart, villeFieldText.indexOf('}', jsonStart) + 1));
    expect(['Paris', 'Lyon']).toContain(villeJson.name);
  });
});
