// With authentication on, the admins of a group manage who belongs to it. The shared instance runs without
// authentication, so the browser is told that it is on and the sign-in is answered for a group admin; every other
// call, the change of members included, reaches the real server, which is where the result is checked.
import { test, expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const API = 'http://localhost:7342/api';

test.describe('Group members', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/groups`, { data: { name: 'payments', code: '', admins: [], members: [] } });
    // Without authentication, the creator recorded as admin is "anonymous": the people are set afterwards.
    const people = await request.put(`${API}/groups/payments/members`, {
      data: { admins: ['alice'], members: ['bob'] },
    });
    expect(people.ok()).toBe(true);
  });

  test('a group admin adds a member, who is saved with the group', async ({ page, request }) => {
    await page.route('**/api/auth/status', (route) =>
      route.fulfill({ json: { enabled: true, show_reset_button: false } }),
    );
    await page.route('**/api/auth/login', (route) =>
      route.fulfill({
        json: { access_token: 'stub-token', refresh_token: null, username: 'alice', is_super_admin: false },
      }),
    );

    await page.goto('/');
    await page.getByTestId('login-form-username-input').fill('alice');
    await page.getByTestId('login-form-password-input').fill('not-a-real-password');
    await page.getByTestId('login-form-submit-button').click();
    await expect(page.getByTestId('app-user-badge')).toContainText('alice');

    await page.getByTestId('app-nav-groups-button').click();
    await page.getByTestId('group-manager-manage-button-payments').click();
    await expect(page.getByTestId('removable-list-item-alice')).toBeVisible();
    await expect(page.getByTestId('removable-list-item-bob')).toBeVisible();
    await page.getByTestId('group-manager-new-member-input-payments').fill('carol');
    await page.getByTestId('group-manager-add-member-button-payments').click();
    await expect(page.getByTestId('removable-list-item-carol')).toBeVisible();
    await expect(page.getByTestId('group-manager-new-member-input-payments')).toHaveValue('');
    await docsScreenshot(page, 'group-members.png', '[data-testid="group-manager-card-payments"]');

    const groups = await (await request.get(`${API}/groups`)).json();
    expect(groups).toEqual([
      expect.objectContaining({ name: 'payments', admins: ['alice'], members: ['bob', 'carol'] }),
    ]);
  });
});
