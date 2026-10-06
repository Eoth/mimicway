import { test, expect } from '@playwright/test';

// The page takes its theme before the application has loaded anything: someone who chose the dark theme does not see
// a light page while the runtime configuration and the language catalogue are fetched.
test.describe('Theme at load', () => {
  for (const theme of ['dark', 'light']) {
    test(`a saved ${theme} theme applies before the application mounts`, async ({ page }) => {
      await page.addInitScript((t) => localStorage.setItem('mimicway-theme', t), theme);
      let release;
      const held = new Promise((resolve) => {
        release = resolve;
      });
      await page.route('**/runtime-config.json', async (route) => {
        await held;
        await route.continue();
      });
      await page.goto('/', { waitUntil: 'domcontentloaded' });
      // The application is not mounted yet: it waits for the runtime configuration.
      await expect(page.getByTestId('app-title-button')).toHaveCount(0);
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      release();
      await expect(page.getByTestId('app-title-button')).toBeVisible();
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    });
  }

  test('without a saved theme, the system preference applies before the application mounts', async ({ browser }) => {
    const context = await browser.newContext({ colorScheme: 'dark' });
    const page = await context.newPage();
    let release;
    const held = new Promise((resolve) => {
      release = resolve;
    });
    await page.route('**/runtime-config.json', async (route) => {
      await held;
      await route.continue();
    });
    await page.goto('/', { waitUntil: 'domcontentloaded' });
    await expect(page.getByTestId('app-title-button')).toHaveCount(0);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    release();
    await expect(page.getByTestId('app-title-button')).toBeVisible();
    await context.close();
  });
});
