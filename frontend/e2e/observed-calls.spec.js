import { test, expect } from './fixtures.mjs';
import http from 'node:http';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const API = 'http://localhost:7342/api';

// Scroll bars take room at narrow widths: this file keeps them, which a launch option does for a whole file only.
test.use({ launchOptions: { ignoreDefaultArgs: ['--hide-scrollbars'] } });

// A SOAP backend that answers with a known size (Mimicway observes only a body of known size) and remembers each
// request it received, on `port` (0 for any).
function startBackend(port) {
  const received = [];
  return new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      const chunks = [];
      req.on('data', (c) => chunks.push(c));
      req.on('end', () => {
        received.push({
          method: req.method,
          url: req.url,
          headers: req.headers,
          body: Buffer.concat(chunks).toString(),
        });
        const body = '<Envelope><Body><GetStatusResponse><status>OPEN</status></GetStatusResponse></Body></Envelope>';
        res.writeHead(200, { 'content-type': 'text/xml', 'content-length': Buffer.byteLength(body) });
        res.end(body);
      });
    });
    server.listen(port, '127.0.0.1', () => resolve({ server, received }));
  });
}

const close = (server) => new Promise((resolve) => server.close(resolve));

// A body with what a shell would read: a quote, a variable, a command substitution, a line break, non-ASCII text.
const ENVELOPE = "<Envelope><Body><GetStatus><ref>it's $HOME `id`\nété</ref></GetStatus></Body></Envelope>";

test.describe('The calls of an observed service', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('a call relayed to a stopped backend is listed with its request and why, and its copied command replays it once the backend is back', async ({
    page,
    context,
    request,
  }) => {
    test.setTimeout(60000);
    // A port with nothing behind it: a backend started then stopped, started again later on the same port.
    const first = await startBackend(0);
    const port = first.server.address().port;
    await close(first.server);

    const created = await request.post(`${API}/services`, {
      data: {
        name: 'soap-proxy',
        listen_path: '',
        real_target_url: `http://127.0.0.1:${port}`,
        is_mocked: false,
        rewrite_directory_urls: false,
        group_name: null,
        wsdl_mode: 'auto',
        rules: [],
      },
    });
    expect(created.ok()).toBe(true);
    expect((await request.post(`${API}/services/soap-proxy/observe`)).status()).toBe(204);
    const relayed = await request.post('http://localhost:7342/soap-proxy/ws?client=e2e', {
      headers: { authorization: 'Bearer e2e-secret', 'content-type': 'text/xml; charset=utf-8' },
      data: ENVELOPE,
    });
    expect(relayed.status()).toBe(502);

    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.goto('/');
    await page.getByText('No group').click();
    await page.getByTestId('service-card-configure-button-soap-proxy').click();
    await page.getByTestId('observed-calls-refresh-button-soap-proxy').click();
    const row = page.locator('[data-testid^="observed-call-soap-proxy-"]');
    await expect(row).toHaveCount(1);
    await expect(row).toContainText('No response: backend unreachable');
    await expect(row).toContainText('/soap-proxy/ws?client=e2e');

    // A disclosure: its name stays, aria-expanded says whether the call is open.
    const toggle = row.getByRole('button', { name: 'Details of the call POST /soap-proxy/ws?client=e2e' });
    await toggle.press('Enter');
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await expect(row.getByText('The backend could not be reached')).toBeVisible();
    await expect(row.locator('[data-testid^="observed-call-address-"]')).toHaveText('POST /soap-proxy/ws?client=e2e');
    await expect(row.locator('[data-testid^="observed-call-request-body-"]')).toHaveText(ENVELOPE);
    await expect(row.getByText('authorization', { exact: true })).toBeVisible();
    await expect(row).not.toContainText('e2e-secret');

    await row.getByRole('button', { name: 'Copy the command' }).click();
    await expect(row.getByText('Command copied.')).toBeVisible();
    const command = await page.evaluate(() => navigator.clipboard.readText());
    expect(command).toContain("'authorization: [redacted]'");
    expect(command).not.toContain('e2e-secret');

    // The backend is back: the command copied sends the same request through Mimicway, which relays it.
    const back = await startBackend(port);
    try {
      // Not a synchronous call: the backend answers from this same process.
      const { stdout: answer } = await promisify(execFile)('sh', ['-c', `${command} --silent --show-error`]);
      expect(answer).toContain('<status>OPEN</status>');
      expect(back.received).toHaveLength(1);
      const replayed = back.received[0];
      expect(replayed.method).toBe('POST');
      expect(replayed.url).toBe('/ws?client=e2e');
      expect(replayed.body).toBe(ENVELOPE);
      expect(replayed.headers['content-type']).toBe('text/xml; charset=utf-8');
      expect(replayed.headers.authorization).toBe('[redacted]');
    } finally {
      await close(back.server);
    }

    // The replay was observed too: the most recent call got the backend's answer, and the failed one is still kept.
    await page.getByTestId('observed-calls-refresh-button-soap-proxy').click();
    await expect(row).toHaveCount(2);
    await expect(row.first()).toContainText('200');
    await expect(row.nth(1)).toContainText('No response: backend unreachable');

    // Down to 320 px nothing scrolls sideways, a long address is cut in its line and whole in the details, and the
    // details button stays in view; the screen ends with a call open, judged by axe in both themes.
    const long = `/soap-proxy/${'segment/'.repeat(40)}end`;
    await request.get(`http://localhost:7342${long}`);
    await page.getByTestId('observed-calls-refresh-button-soap-proxy').click();
    await expect(row).toHaveCount(3);
    for (const width of [1280, 768, 320]) {
      await page.setViewportSize({ width, height: 800 });
      const widths = await page.evaluate(() => [document.documentElement.scrollWidth, window.innerWidth]);
      expect(widths[0], `page ${widths[0]} px wide in a ${widths[1]} px window`).toBeLessThanOrEqual(widths[1]);
      for (let i = 0; i < 3; i++) {
        const box = await row.nth(i).locator('.call-toggle').boundingBox();
        expect(box.x + box.width, `details button of call ${i} at ${width} px`).toBeLessThanOrEqual(widths[1]);
      }
    }
    const longToggle = row.first().locator('.call-toggle');
    await longToggle.click();
    await expect(row.first().locator('[data-testid^="observed-call-address-"]')).toHaveText(`GET ${long}`);
    const widths = await page.evaluate(() => [document.documentElement.scrollWidth, window.innerWidth]);
    expect(widths[0]).toBeLessThanOrEqual(widths[1]);
  });
});
