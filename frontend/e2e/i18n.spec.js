import { test, expect } from './fixtures.mjs';

// The interface follows the browser's language (English unless a catalogue exists for it) and the language picked
// in the header, which is remembered.
test.describe('Interface language', () => {
  test.use({ locale: 'en-US' });

  test('an English browser gets the English interface and can switch to French', async ({ page, request }) => {
    await request.delete('http://localhost:7342/api/config/reset');
    await page.goto('/');
    await expect(page.locator('html')).toHaveAttribute('lang', 'en');
    await expect(page.locator('[data-testid="app-add-service-button"]')).toHaveText('+ Add a service');
    await expect(page.locator('[data-testid="app-nav-groups-button"]')).toHaveText('Groups');

    await page.locator('[data-testid="app-language-select"]').selectOption('fr');
    await expect(page.locator('[data-testid="app-add-service-button"]')).toHaveText('+ Ajouter un service');
    await expect(page.locator('html')).toHaveAttribute('lang', 'fr');

    await page.reload();
    await expect(page.locator('[data-testid="app-nav-groups-button"]')).toHaveText('Groupes');
  });
});
