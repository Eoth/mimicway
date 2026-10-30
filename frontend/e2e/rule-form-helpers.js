// Helpers of the specs that open the rule form of a service created through the API.

export function validService(name, overrides = {}) {
  return {
    name,
    listen_path: '/e2e/*',
    real_target_url: 'http://e2e:80',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
    ...overrides,
  };
}

export async function openAddRuleForm(page) {
  await page.goto('/');
  await page.waitForLoadState('networkidle');
  const group = page.locator('button[aria-expanded]').first();
  if ((await group.getAttribute('aria-expanded')) === 'false') {
    await group.click();
  }
  await page
    .getByRole('button', { name: /Configure/ })
    .first()
    .click();
  await page.getByRole('button', { name: /Add a rule/ }).click();
}
