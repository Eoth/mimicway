// Accessibility of every screen the suite reaches, judged by axe-core with the WCAG 2.1 A and AA rules. Every spec
// takes `test` and `expect` from here, not from Playwright (scripts/e2e-fixtures.test.mjs checks it), so the screens
// come from the suite itself: an init script watches the page, and once it has been still for a moment, the screen it
// shows (the view, `data-view` of <main>, the forms and the dialogs open) is copied: its markup, with the values of its
// fields and without its scripts. While the test runs, the fixture only reads the page; axe-core, which scrolls to
// measure contrasts, never runs beside the test. When the test ends, each copy is judged in a page of its own in both
// themes, and the last screen, live, in the other language too. Each screen is judged once per run in each theme and
// language; a violation fails the test that showed it, unless `a11y-exceptions.json` lists it until a date not yet
// passed.
//
// What it does not see: a screen in a language the suite never shows it in, unless it is the last screen of a test; a
// second state of the same screen (more services, a section unfolded), judged the first time only; hover and focus
// styles, what is shown only while a field has the focus (the suggestions of the script editor), and what a copy
// cannot hold (scroll positions, a dialog opened with showModal); a screen that never stays still while the test is on
// it, other than a last screen, and a last screen that could not be switched to a language (each one is named in the
// test's annotations); pages without the interface (a mock's response opened in the browser); and what axe-core
// cannot judge: whether a text alternative says the right thing, the keyboard order, where the focus goes after an
// action (rule-form-keyboard.spec.js checks some by hand).
import { test as base, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { EXCEPTIONS } from './a11y-exceptions.mjs';

export { expect };

const TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'];
const STILL_MS = 50;
const THEMES = ['light', 'dark'];
const LANGUAGES = ['en', 'fr'];

const excepted = (violation, node, view) =>
  EXCEPTIONS.some((e) => e.rule === violation.id && e.screen === view && node.target.join(' ') === e.target);

// Runs in the page, before its own scripts. Counts changes, names the screen shown, and calls back once the page has
// been still for STILL_MS. Defined here and never built from a text: the interface's policy forbids eval.
function watchScreens(stillMs) {
  window.__a11yChanges = 0;
  window.__a11yScreen = () => {
    const view = document.querySelector('main[data-view]')?.dataset.view;
    if (!view) return null;
    const dialogs = [...document.querySelectorAll('[role="dialog"], [role="alertdialog"], dialog[open]')]
      .map((d) => d.dataset.testid || 'dialog')
      .sort();
    // A form open within the view (a rule form in a service's details) is a screen of its own.
    const forms = [...new Set([...document.querySelectorAll('main form')].map((f) => f.classList[0] || 'form'))].sort();
    return [view, ...forms, ...dialogs].join(' + ');
  };
  let timer;
  new MutationObserver(() => {
    window.__a11yChanges += 1;
    clearTimeout(timer);
    timer = setTimeout(() => {
      const screen = window.__a11yScreen();
      if (screen) window.__a11yStill?.(screen);
    }, stillMs);
  }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
}

// The markup of the page as shown: field values written into it, scripts left out so that the copy stays as it is.
function copyPage() {
  const copy = document.documentElement.cloneNode(true);
  const fields = (root) => root.querySelectorAll('input, textarea, select');
  const copied = fields(copy);
  fields(document.documentElement).forEach((field, i) => {
    const target = copied[i];
    if (field.tagName === 'TEXTAREA') target.textContent = field.value;
    else if (field.tagName === 'SELECT') {
      [...field.options].forEach((option, j) => target.options[j]?.toggleAttribute('selected', option.selected));
    } else if (field.type === 'checkbox' || field.type === 'radio') target.toggleAttribute('checked', field.checked);
    else if (field.type !== 'file') target.setAttribute('value', field.value);
  });
  copy.querySelectorAll('script').forEach((script) => script.remove());
  return {
    html: `<!doctype html>${copy.outerHTML}`,
    lang: document.documentElement.lang,
    changes: window.__a11yChanges,
  };
}

const seen = new Set();
const key = (screen, theme, lang) => `${screen} (${theme}, ${lang})`;

async function judge(page, screen, lang, violations) {
  const theme = await page.evaluate(() => document.documentElement.dataset.theme);
  const id = key(screen, theme, lang);
  if (seen.has(id)) return;
  // Colors read during a transition (the theme just switched) mix two states: wait for the end of every finite one.
  await page
    .waitForFunction(
      () =>
        document
          .getAnimations()
          .every((a) => a.playState !== 'running' || a.effect?.getComputedTiming().iterations === Infinity),
      null,
      { timeout: 2000 },
    )
    .catch(() => {});
  const results = await new AxeBuilder({ page }).withTags(TAGS).analyze();
  seen.add(id);
  const view = screen.split(' + ')[0];
  for (const violation of results.violations) {
    const nodes = violation.nodes.filter((node) => !excepted(violation, node, view));
    if (nodes.length > 0) violations.push({ id, violation, nodes });
  }
}

const setTheme = (page, theme) =>
  page.evaluate((name) => document.documentElement.setAttribute('data-theme', name), theme);

export const test = base.extend({
  page: async ({ page }, use, testInfo) => {
    const copies = new Map();
    const unsettled = new Set();
    let queue = Promise.resolve();

    async function keep(screen) {
      const lang = await page.evaluate(() => document.documentElement.lang);
      const id = `${screen} (${lang})`;
      if (copies.has(id) || THEMES.every((theme) => seen.has(key(screen, theme, lang)))) return;
      const copy = await page.evaluate(copyPage);
      const now = await page.evaluate(() => ({ screen: window.__a11yScreen(), changes: window.__a11yChanges }));
      if (now.screen !== screen || now.changes !== copy.changes) {
        unsettled.add(id);
        return;
      }
      unsettled.delete(id);
      copies.set(id, { screen, url: page.url(), ...copy });
    }

    await page.exposeBinding('__a11yStill', (_source, screen) => {
      queue = queue.then(() => keep(screen)).catch(() => {});
    });
    await page.addInitScript(watchScreens, STILL_MS);

    await use(page);

    await queue;
    const violations = [];
    if (copies.size > 0) {
      const judged = await page.context().newPage();
      for (const { screen, url, html, lang } of copies.values()) {
        const base = `<base href="${new URL('/', url).href}">`;
        await judged.setContent(
          html.replace(/<head([^>]*)>/i, (head) => `${head}${base}`),
          { waitUntil: 'load' },
        );
        for (const theme of THEMES) {
          await setTheme(judged, theme);
          await judge(judged, screen, lang, violations);
        }
      }
      await judged.close();
    }

    const last = await page.evaluate(() => window.__a11yScreen?.() ?? null).catch(() => null);
    if (last) {
      for (const lang of LANGUAGES) {
        // A spec that served the interface from a server of its own may have stopped it: no catalogue to load.
        const switched = await page
          .getByTestId('app-language-select')
          .selectOption(lang, { force: true, timeout: 2000 })
          .then(() => page.waitForFunction((code) => document.documentElement.lang === code, lang, { timeout: 2000 }))
          .then(() => true)
          .catch(() => false);
        if (!switched) {
          testInfo.annotations.push({
            type: 'accessibility',
            description: `last screen not shown in ${lang}: ${last}`,
          });
          continue;
        }
        for (const theme of THEMES) {
          await setTheme(page, theme);
          await judge(page, last, lang, violations);
        }
      }
    }
    for (const id of unsettled) {
      testInfo.annotations.push({ type: 'accessibility', description: `never still long enough to copy: ${id}` });
    }
    if (violations.length > 0) {
      const lines = violations.flatMap(({ id, violation, nodes }) => [
        `${id}: ${violation.id} (${violation.impact}) ${violation.help}`,
        ...nodes
          .slice(0, 5)
          .map((node) => `    ${node.target.join(' ')}: ${node.failureSummary.split('\n').join(' ')}`),
      ]);
      throw new Error(`Accessibility violations (WCAG 2.1 A and AA, axe-core):\n${lines.join('\n')}`);
    }
  },
});
