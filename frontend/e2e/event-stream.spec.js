import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const API = 'http://localhost:7342/api';
const BASE = 'http://localhost:7342';

// A rule that answers with server-sent events, written in the rule form and then read as a client of such a stream
// reads it: chunk by chunk, each event at the time it arrives, then the end of the response, or a transport error
// where the rule cuts the stream.

const pipeline = {
  name: 'pipeline',
  listen_path: '',
  real_target_url: '',
  is_mocked: true,
  rewrite_directory_urls: false,
  group_name: null,
  wsdl_mode: 'auto',
  rules: [],
};

async function openService(page, serviceName) {
  await page.goto('/');
  await page.waitForLoadState('networkidle');
  // Ungrouped services sit in a folded group of their own.
  const group = page.locator('button[aria-expanded]').first();
  if ((await group.count()) > 0 && (await group.getAttribute('aria-expanded')) === 'false') {
    await group.click();
  }
  await page.getByRole('button', { name: new RegExp(`Configure the service ${serviceName}`) }).click();
}

// The events of `url`, each with the milliseconds since the response headers, and the error that ended the body if
// it did not end. Read by Node's fetch, outside the page.
async function readStream(url) {
  const response = await fetch(url);
  const opened = Date.now();
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const events = [];
  let pending = '';
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return { response, events, error: null };
      pending += decoder.decode(value, { stream: true });
      let end;
      while ((end = pending.indexOf('\n\n')) >= 0) {
        events.push({ text: pending.slice(0, end + 1), at: Date.now() - opened });
        pending = pending.slice(end + 2);
      }
    }
  } catch (error) {
    return { response, events, error };
  }
}

test.describe('A rule that answers with server-sent events', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, { data: pipeline });
  });

  test('is written as a list of events in the rule form, and streamed in order with its delays', async ({ page }) => {
    await openService(page, 'pipeline');
    await page.getByRole('button', { name: /Add a rule/ }).click();
    await page.getByTestId('rule-form-name-input').fill('log');
    await page.getByTestId('rule-form-subpath-input').fill('/log/{run}');
    await page.getByTestId('rule-form-mode-button-events').click();

    await page.getByLabel('Data').first().fill('pulling image');
    await page.getByTestId('rule-events-add-button').click();
    await page.getByTestId('rule-events-delay-input-1').fill('400');
    await page.getByTestId('rule-events-data-input-1').fill('run {{path.run}}\ncopied 3, failed: 0');
    await page.getByTestId('rule-events-add-button').click();
    await page.getByTestId('rule-events-name-input-2').fill('endofstream');
    await page.getByTestId('rule-events-delay-input-2').fill('0');
    await page.getByTestId('rule-events-data-input-2').fill('endofstream');
    await docsScreenshot(page, 'rule-event-stream.png', '[data-testid^="rule-events-row-"]');
    await page.getByTestId('rule-form-submit-button').click();
    await expect(page.getByTestId('rule-list-edit-button-log')).toBeVisible();

    const { response, events, error } = await readStream(`${BASE}/pipeline/log/42`);
    expect(response.headers.get('content-type')).toBe('text/event-stream');
    expect(error).toBeNull();
    expect(events.map((e) => e.text)).toEqual([
      'data: pulling image\n',
      'data: run 42\ndata: copied 3, failed: 0\n',
      'event: endofstream\ndata: endofstream\n',
    ]);
    expect(events[1].at - events[0].at).toBeGreaterThanOrEqual(380);
  });

  test('is cut where the rule form says, and the client gets a transport error', async ({ page, request }) => {
    await request.put(`${API}/services/pipeline`, {
      data: {
        ...pipeline,
        rules: [
          {
            name: 'log',
            method: 'GET',
            sub_path: '/log/{run}',
            action: 'mock',
            conditions: { all_of: [], any_of: [] },
            response_mode: 'events',
            response: {
              status: 200,
              headers: [],
              body: [],
              events: {
                events: [
                  { data: 'pulling image', delay_ms: 0 },
                  { data: 'copied 3, failed: 0', delay_ms: 100 },
                  { event: 'endofstream', data: 'endofstream', delay_ms: 0 },
                ],
              },
            },
          },
        ],
      },
    });
    await openService(page, 'pipeline');
    await page.getByTestId('rule-list-edit-button-log').click();
    await page.getByTestId('rule-events-end-select').selectOption({ label: 'Cut instead of event 2' });
    await page.getByTestId('rule-form-submit-button').click();
    await expect(page.getByTestId('rule-list-edit-button-log')).toBeVisible();

    const { events, error } = await readStream(`${BASE}/pipeline/log/7`);
    expect(events.map((e) => e.text)).toEqual(['data: pulling image\n']);
    expect(error).not.toBeNull();

    // The request log records the stream once, with what it sent and how it finished.
    const logs = await (await request.get(`${API}/logs?limit=5`)).json();
    const entry = logs.find((l) => l.path === '/pipeline/log/7');
    expect(entry.stream).toMatchObject({ planned: 2, sent_count: 1, outcome: 'cut' });
  });
});
