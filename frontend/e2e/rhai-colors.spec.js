import { test, expect } from '@playwright/test';
import { openAddRuleForm, validService } from './rule-form-helpers.js';

const API = 'http://localhost:7342/api';

// Headless Chromium hides scroll bars unless told otherwise, and the room a scroll bar takes is what narrows the lines
// of a field: this file keeps them, which a launch option does for a whole file only.
test.use({ launchOptions: { ignoreDefaultArgs: ['--hide-scrollbars'] } });

test.beforeEach(async ({ request }) => {
  await request.delete(`${API}/config/reset`);
  await request.post(`${API}/services`, { data: validService('colors-svc') });
});

// The colors of a script are a copy of its text laid under the textarea, whose own text is transparent. The browser
// judges that they line up: the same text, the same box, the same room for the lines, the same properties that place
// a character, and the same scroll once the script is taller than the field.
test('the colors of a script lie exactly under its text, scrolled with it', async ({ page }) => {
  await openAddRuleForm(page);
  await page.locator('input#rule-name').fill('colors');
  await page.getByRole('switch', { name: 'Custom script' }).click();

  const scriptField = page.locator('#rule-script');
  const colors = page.getByTestId('rhai-script-editor-colors-rule-script');
  const longLine = `let note = "${'a long text that wraps '.repeat(8)}"; // and a comment`;
  const script = [
    'let total = 0;',
    longLine,
    ...Array.from({ length: 12 }, (_, i) => `total += parse_json(request.body).items[${i}].len(); /* ${i} */`),
    'if total > 10 { "many" } else { `few` }',
  ].join('\n');
  await scriptField.fill(script);

  await expect(colors).toHaveText(`${script} `, { useInnerText: false });
  await expect(colors.locator('.rhai-keyword').first()).toHaveText('let');
  await expect(colors.locator('.rhai-function').first()).toHaveText('parse_json');

  const placing = [
    'font-family',
    'font-size',
    'font-weight',
    'font-style',
    'font-variant-ligatures',
    'line-height',
    'letter-spacing',
    'word-spacing',
    'tab-size',
    'white-space',
    'overflow-wrap',
    'box-sizing',
    'scrollbar-gutter',
    ...['top', 'right', 'bottom', 'left'].flatMap((side) => [`padding-${side}`, `border-${side}-width`]),
  ];
  const measure = (el, properties) => {
    const style = getComputedStyle(el);
    const box = el.getBoundingClientRect();
    return {
      style: Object.fromEntries(properties.map((p) => [p, style.getPropertyValue(p)])),
      box: [box.x, box.y, box.width, box.height],
      lineRoom: el.clientWidth,
      scroll: [el.scrollTop, el.scrollHeight],
    };
  };
  const field = await scriptField.evaluate(measure, placing);
  const copy = await colors.evaluate(measure, placing);
  expect(copy.style).toEqual(field.style);
  expect(copy.box).toEqual(field.box);
  expect(copy.lineRoom).toBe(field.lineRoom);
  expect(field.scroll[1]).toBeGreaterThan(field.box[3]);
  expect(copy.scroll[1]).toBe(field.scroll[1]);

  await scriptField.evaluate((el) => {
    el.scrollTop = el.scrollHeight;
    el.dispatchEvent(new Event('scroll'));
  });
  const scrolled = await scriptField.evaluate((el) => el.scrollTop);
  expect(scrolled).toBeGreaterThan(0);
  await expect.poll(() => colors.evaluate((el) => el.scrollTop)).toBe(scrolled);
});
