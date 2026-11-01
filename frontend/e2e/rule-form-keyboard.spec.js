import { test, expect } from './fixtures.mjs';

const API = 'http://localhost:7342/api';

// The action of a rule (mock or proxy) is a radio group shown as two cards: the keyboard must reach it like any radio
// group, Tab landing on the checked option and the arrow keys moving the choice.
test.describe('Rule form: the action is chosen with the keyboard', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('Tab reaches the checked action, and an arrow key switches it to proxy', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: {
        name: 'keyboard-svc',
        listen_path: '',
        real_target_url: 'http://backend:8080',
        is_mocked: true,
        rewrite_directory_urls: false,
        group_name: null,
        wsdl_mode: 'auto',
        rules: [],
      },
    });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    const group = page.locator('button[aria-expanded]').first();
    if ((await group.getAttribute('aria-expanded')) === 'false') await group.click();
    await page.getByRole('button', { name: /Configure the service keyboard-svc/ }).click();
    await page.getByRole('button', { name: /Add a rule/ }).click();

    await page.getByTestId('rule-form-subpath-input').focus();
    await page.keyboard.press('Tab');
    await expect(page.getByTestId('rule-form-action-mock-radio')).toBeFocused();

    await page.keyboard.press('ArrowRight');
    await expect(page.getByTestId('rule-form-action-proxy-radio')).toBeChecked();
    await expect(page.getByTestId('rule-form-action-proxy-option')).toHaveClass(/selected/);
  });
});
