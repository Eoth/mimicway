import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const API = 'http://localhost:7342/api';

// These tests run against a server built with `--features messaging-kafka` only (otherwise /api/messaging/* answers
// 404), but need no Kafka broker: a simulated message (POST /api/messaging/simulate) goes through the same matching,
// rendering and log as a message read from Kafka (process_message, src/messaging/consumer.rs).
async function messagingAvailable(request) {
  const res = await request.get(`${API}/messaging/status`);
  if (!res.ok()) return false;
  const body = await res.json();
  return !!body.available;
}

function messagingService(name, overrides = {}) {
  return {
    name,
    listen_path: '/v1/*',
    real_target_url: 'http://backend:8080',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [
      {
        // The rule model is shared with HTTP, where method is required (validate_service checks it against
        // VALID_METHODS, which has no ANY); matching a message ignores it (src/messaging/matcher.rs): any valid method
        // does.
        name: 'order-created',
        method: 'POST',
        sub_path: null,
        action: 'mock',
        pre_script: null,
        script: null,
        post_script: null,
        conditions: {
          all_of: [{ source: { type: 'JsonPointer', key: '/type' }, operator: { type: 'Eq', value: 'order.created' } }],
          any_of: [],
        },
        response: {
          status: 200,
          headers: [],
          body: [{ type: 'Literal', value: 'order-ack' }],
          chaos: null,
        },
      },
    ],
    ...overrides,
  };
}

test.describe('Messaging (Kafka): the message log, fed by simulated messages', () => {
  test.beforeEach(async ({ request }) => {
    const available = await messagingAvailable(request);
    test.skip(!available, 'backend not built with --features messaging-kafka');
    await request.delete(`${API}/config/reset`);
  });

  test('a simulated message that matches shows in the log with its service and rule', async ({ page, request }) => {
    const created = await request.post(`${API}/services`, { data: messagingService('kafka-svc') });
    expect(created.ok()).toBe(true);

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await docsScreenshot(page, 'kafka-nav-button.png', '[data-testid="app-nav-messaging-button"]');
    await page.getByTitle('Kafka message log').click();
    await expect(page.getByRole('heading', { name: 'Kafka messages' })).toBeVisible();

    await page.getByLabel('Topic of the simulated message').fill('orders.in');
    await page.getByLabel('Body of the simulated message').fill('{"type":"order.created"}');
    await docsScreenshot(
      page,
      'kafka-simulation-form.png',
      '[data-testid="messaging-log-sim-topic-input"], [data-testid="messaging-log-simulate-button"]',
    );
    await page.getByRole('button', { name: 'Simulate' }).click();

    // The message log outlives a configuration reset, and lists the newest message first: on a server that already ran
    // this suite, the row of this run is the first one with its topic.
    const row = page.locator('tr', { hasText: 'orders.in' }).first();
    await expect(row).toBeVisible();
    await expect(row).toContainText('kafka-svc / order-created');
    await expect(row).toContainText('Matches');
    await expect(page.getByTestId('notification')).toBeHidden();
    await docsScreenshot(page, 'kafka-message-log.png', '[data-testid^="messaging-log-row-"]');
  });

  test('a message that no rule matches is logged as such', async ({ page, request }) => {
    const created = await request.post(`${API}/services`, { data: messagingService('kafka-svc-2') });
    expect(created.ok()).toBe(true);

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.getByTitle('Kafka message log').click();

    await page.getByLabel('Topic of the simulated message').fill('orders.unmatched');
    await page.getByLabel('Body of the simulated message').fill('{"type":"order.cancelled"}');
    await page.getByRole('button', { name: 'Simulate' }).click();

    const row = page.locator('tr', { hasText: 'orders.unmatched' }).first();
    await expect(row).toBeVisible();
    await expect(row).toContainText('Does not match');
  });

  test('a large message body is logged with the Truncated badge', async ({ page, request }) => {
    test.setTimeout(30000);
    const created = await request.post(`${API}/services`, { data: messagingService('kafka-svc-3') });
    expect(created.ok()).toBe(true);

    const bigPayload = JSON.stringify({ type: 'order.created', filler: 'x'.repeat(20000) });

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.getByTitle('Kafka message log').click();

    await page.getByLabel('Topic of the simulated message').fill('orders.big');
    await page.getByLabel('Body of the simulated message').fill(bigPayload);
    await page.getByRole('button', { name: 'Simulate' }).click();

    const row = page.locator('tr', { hasText: 'orders.big' }).first();
    await expect(row).toBeVisible();
    await expect(row).toContainText('Truncated');

    // The details give the real size, though the logged body is truncated.
    await row.locator('.btn-detail').click({ timeout: 20000 });
    const dialog = page.getByRole('dialog', { name: 'Message details' });
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(`${bigPayload.length} bytes`);
  });

  test('simulating without a topic shows an error', async ({ page, request }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.getByTitle('Kafka message log').click();

    await page.getByRole('button', { name: 'Simulate' }).click();
    await expect(page.getByText('A topic is required to simulate a message.')).toBeVisible();
  });
});
