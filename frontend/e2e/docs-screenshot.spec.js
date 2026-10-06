// The framing guard of the documentation screenshots (docs-screenshot.js), on a page built for the purpose: no
// server is involved. It runs in the standard suite, where docsScreenshot() frames and checks without capturing; it
// is skipped when the screenshots are regenerated, where a passing call would write an image of this page into the
// guide.
import { test, expect } from '@playwright/test';
import { docsScreenshot, DOCS_SCREENSHOTS_ENABLED } from './docs-screenshot.js';
import { runScenario } from './scenario-runner.js';

test.skip(DOCS_SCREENSHOTS_ENABLED, 'checks the guard, which writes no image of the guide');

const SCRIPT = '[data-testid="rhai-script-editor-textarea-demo"]';
const PAGE = `<!doctype html>
<body style="margin: 0">
  <h1 id="top">Top of the page</h1>
  <textarea data-testid="rhai-script-editor-textarea-demo" rows="2">line 1
line 2
line 3
line 4
line 5
line 6</textarea>
  <div style="height: 2000px"></div>
  <p id="below">Below the fold</p>
  <div id="tall" style="height: 1500px">Taller than the view</div>
  <p id="hidden" hidden>Hidden</p>
</body>`;

test.describe('Documentation screenshots: the image must show its whole subject', () => {
  test.beforeEach(async ({ page }) => {
    await page.setContent(PAGE);
  });

  test('a subject below the fold is scrolled into view', async ({ page }) => {
    await docsScreenshot(page, 'below.png', '#below');
    const box = await page.locator('#below').boundingBox();
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.y + box.height).toBeLessThanOrEqual(page.viewportSize().height);
  });

  test('a subject already in view is not scrolled', async ({ page }) => {
    await docsScreenshot(page, 'top.png', '#top');
    expect(await page.evaluate(() => window.scrollY)).toBe(0);
  });

  test('a subject taller than the view fails the capture', async ({ page }) => {
    await expect(docsScreenshot(page, 'tall.png', '#tall')).rejects.toThrow(
      /#tall is not entirely in the captured view/,
    );
  });

  test('elements of a subject too far apart to show together fail the capture', async ({ page }) => {
    await expect(docsScreenshot(page, 'apart.png', '#top, #below')).rejects.toThrow(
      /is not entirely in the captured view/,
    );
  });

  test('a text area that hides part of its text fails the capture, until it is resized to its content', async ({
    page,
  }) => {
    await expect(docsScreenshot(page, 'script.png', SCRIPT)).rejects.toThrow(/hides part of its content/);
    const resize = { action: 'resizeToContent', target: 'rhaiScriptEditor.textarea', params: { id: 'demo' } };
    await runScenario(page, { scenario: 'Show the whole script', steps: [resize] });
    await docsScreenshot(page, 'script.png', SCRIPT);
  });

  test('a missing or hidden subject fails the capture', async ({ page }) => {
    await expect(docsScreenshot(page, 'missing.png', '#nowhere')).rejects.toThrow(/#nowhere matches nothing/);
    await expect(docsScreenshot(page, 'hidden.png', '#hidden')).rejects.toThrow(/#hidden is not displayed/);
  });

  test('a capture without a subject fails', async ({ page }) => {
    await expect(docsScreenshot(page, 'none.png')).rejects.toThrow(/name the subject of the image/);
  });

  test('a scenario screenshot step without a target fails', async ({ page }) => {
    const scenario = { scenario: 'No subject', steps: [{ action: 'screenshot', file: 'none.png' }] };
    await expect(runScenario(page, scenario)).rejects.toThrow(/"target" \(the subject of the image\) is required/);
  });
});
