import { readFileSync } from 'node:fs';
import { test, expect } from './fixtures.mjs';

// The foot of the page says which version of Mimicway answers, as the server itself tells it: what a tester quotes when
// reporting a problem. The version of the interface's package is the one a release checks against the binary's.
test('the foot of the page shows the version of the server', async ({ page, request }) => {
  const answer = await request.get('http://localhost:7342/api/version');
  expect(answer.status()).toBe(200);
  const { version } = await answer.json();
  const released = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')).version;
  expect(version).toBe(released);

  await page.goto('/');
  // In a footer of the page, after its main content: a landmark of its own, outside the header and the content.
  await expect(page.locator('main ~ footer [data-testid="app-version"]')).toHaveText(`Version ${version}`);
});
