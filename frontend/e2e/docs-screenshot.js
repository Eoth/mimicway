// Screenshots of the user guide, taken by the end-to-end suite so that they follow the interface instead of going
// stale. The guide exists once per language, in docs/<language>/ with the same file names, so each call writes the
// same state once per language: the page is captured in English (the language the suite runs in), switched to French
// with the interface's own language selector, captured again, then switched back so that the test goes on in English.
// The switch does not reload the page, so both images show exactly the same state, and the production bundle needs no
// test hook.
//
// Every image names its subject, the element(s) it is taken to show. The subject is scrolled into view when it is not
// entirely there, and the call fails when it still is not, in any language: an image cut before its subject is
// refused rather than published. This framing runs in every run, the standard one included, so that CI catches a
// subject pushed out of view by a change of the interface, without taking any screenshot.
//
// Capturing is disabled by default: it only happens when DOCS_SCREENSHOTS is set, which
// playwright.docs-screenshots.config.js does and playwright.config.js never does, so the standard run writes nothing.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DOCS_DIR = path.join(__dirname, '..', '..', 'docs');
const LANGUAGE_SELECT = '[data-testid="app-language-select"]';
const NOTIFICATION = '[data-testid="notification"]';
// The language of the suite first; the others are captured by switching to them.
const LANGUAGES = [
  { code: 'en', locale: 'en-US' },
  { code: 'fr', locale: 'fr-FR' },
];

export const DOCS_SCREENSHOTS_ENABLED = !!process.env.DOCS_SCREENSHOTS;

function target(language, filename) {
  const dir = path.join(DOCS_DIR, language, 'screenshots');
  fs.mkdirSync(dir, { recursive: true });
  return path.join(dir, filename);
}

const nextFrames = (page) =>
  page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));

// Transitions (a group's chevron, a button turning from primary to outline) are run to their end, and the pointer is
// moved off the content: otherwise an image may catch a transition half-way, or a button hovered in one language only
// (the texts, hence the layout, differ), and the two images of a pair would not show the same state.
async function capture(page, language, filename) {
  await page.mouse.move(0, 0);
  await page.screenshot({ path: target(language, filename), animations: 'disabled' });
}

// Whether an element shows in the captured viewport (an element scrolled out of it does not appear in the image).
async function inViewport(page, selector) {
  const element = page.locator(selector);
  if (!(await element.isVisible())) return false;
  const box = await element.boundingBox();
  const { height } = page.viewportSize();
  return box !== null && box.y + box.height > 0 && box.y < height;
}

// The box that holds every element of the subject, relative to the viewport, or why there is none.
function subjectBox(selector) {
  const elements = [...document.querySelectorAll(selector)];
  if (elements.length === 0) return { problem: 'matches nothing' };
  const rects = elements.map((element) => element.getBoundingClientRect());
  if (rects.some((rect) => rect.width === 0 || rect.height === 0)) return { problem: 'is not displayed' };
  // A text area or a scrolling panel shorter than its content shows only part of it, wherever it sits.
  const clipped = elements.find(
    (element) => element.scrollHeight > element.clientHeight + 1 && getComputedStyle(element).overflowY !== 'visible',
  );
  if (clipped) {
    return {
      problem: `hides part of its content (${clipped.scrollHeight} px of content in ${clipped.clientHeight} px)`,
    };
  }
  return {
    top: Math.min(...rects.map((rect) => rect.top)),
    bottom: Math.max(...rects.map((rect) => rect.bottom)),
    left: Math.min(...rects.map((rect) => rect.left)),
    right: Math.max(...rects.map((rect) => rect.right)),
    viewportHeight: window.innerHeight,
    viewportWidth: document.documentElement.clientWidth,
  };
}

const fitsInView = (box) =>
  box.top >= 0 && box.left >= 0 && box.bottom <= box.viewportHeight && box.right <= box.viewportWidth;

// Fails unless the whole subject shows in the viewport, which is what the image captures.
async function assertSubjectInView(page, filename, subject, language) {
  const box = await page.evaluate(subjectBox, subject);
  const where = `docsScreenshot(${filename}, ${language}): the subject ${subject}`;
  if (box.problem) throw new Error(`${where} ${box.problem}`);
  if (!fitsInView(box)) {
    throw new Error(
      `${where} is not entirely in the captured view: it spans ${Math.round(box.top)}..${Math.round(box.bottom)} px ` +
        `vertically and ${Math.round(box.left)}..${Math.round(box.right)} px horizontally, the view is ` +
        `${box.viewportWidth}x${box.viewportHeight}`,
    );
  }
}

// Scrolls the subject into view, only when it is not entirely there and only as far as needed (with a small margin):
// a screen already framed keeps its position and its open popups, and a subject just below the fold keeps the
// context above it, the navigation bar included. scrollIntoView reaches the subject through any scrolling container;
// the window then moves so that the whole subject, not only its first element, ends up in view, its top first.
async function frameSubject(page, filename, subject, language) {
  const box = await page.evaluate(subjectBox, subject);
  if (!box.problem && !fitsInView(box)) {
    await page.evaluate((selector) => {
      const margin = 16;
      const elements = [...document.querySelectorAll(selector)];
      const rects = elements.map((element) => element.getBoundingClientRect());
      const first = elements[rects.findIndex((rect) => rect.top === Math.min(...rects.map((r) => r.top)))];
      first.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
      const after = elements.map((element) => element.getBoundingClientRect());
      const top = Math.min(...after.map((rect) => rect.top));
      const below = Math.max(...after.map((rect) => rect.bottom)) - (window.innerHeight - margin);
      window.scrollBy({
        top: below > 0 ? Math.min(below, top - margin) : Math.min(0, top - margin),
        behavior: 'instant',
      });
    }, subject);
    await nextFrames(page);
  }
  await assertSubjectInView(page, filename, subject, language);
}

async function interfaceLanguage(page) {
  return page.evaluate(() => document.documentElement.lang);
}

async function switchLanguage(page, code) {
  // The selector's own change handler, as a user's choice triggers it, but without selectOption(), which may scroll the
  // navigation bar into view or move the focus: the screen must keep its scroll position and its open popups.
  await page.locator(LANGUAGE_SELECT).evaluate((select, value) => {
    select.value = value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
  }, code);
  // The catalogue is fetched on first use; `lang` changes once it is applied.
  await page.waitForFunction((expected) => document.documentElement.lang === expected, code);
  // Two frames: the re-rendered texts are laid out and painted before the capture.
  await nextFrames(page);
}

// A screen without the navigation bar (the login screen) has no language selector: it is opened again at the same URL
// in a browser set to the other language, which is how a visitor of that language first sees it. Only valid for a
// screen that opening its URL reproduces; `waitFor` is the selector that shows it is ready.
async function captureReopened(page, language, filename, subject, waitFor) {
  const context = await page.context().browser().newContext({ locale: language.locale, viewport: page.viewportSize() });
  try {
    const other = await context.newPage();
    await other.goto(page.url());
    await other.waitForFunction((expected) => document.documentElement.lang === expected, language.code);
    await other.locator(waitFor).waitFor();
    await other.waitForLoadState('networkidle');
    await frameSubject(other, filename, subject, language.code);
    await capture(other, language.code, filename);
  } finally {
    await context.close();
  }
}

/**
 * Frames `subject` (a CSS selector: the element, or the elements, the image is taken to show) and, when
 * DOCS_SCREENSHOTS is set, writes docs/<language>/screenshots/<filename> for each language of the guide.
 *
 * The subject is required: it is scrolled into view when needed, and the call fails when it matches nothing, is not
 * displayed, hides part of its own content (a text area scrolled inside), or does not fit entirely in the view in one
 * of the languages.
 *
 * Texts written before the switch keep their language, so they are refused or redone rather than captured as they are:
 * - a notification holds a sentence chosen when it appeared: a visible one is an error (wait until it closes);
 * - a text worded by the server follows the language of the request that fetched it: `options.afterSwitch(page)` runs
 *   after every switch (back to English included) to fetch it again, as the user would by repeating the action.
 * `options.reopenWaitingFor`: for a screen without the language selector, the selector to wait for once the screen is
 * opened again in the other languages (see captureReopened); without it, a missing selector is an error.
 */
export async function docsScreenshot(page, filename, subject, options = {}) {
  if (typeof subject !== 'string' || subject.trim() === '') {
    throw new Error(`docsScreenshot(${filename}): name the subject of the image, as a CSS selector`);
  }
  await frameSubject(page, filename, subject, LANGUAGES[0].code);
  if (!DOCS_SCREENSHOTS_ENABLED) return;

  const [source, ...others] = LANGUAGES;
  const shown = await interfaceLanguage(page);
  if (shown !== source.code) {
    throw new Error(`docsScreenshot(${filename}): the interface is in "${shown}", expected "${source.code}"`);
  }
  if (await inViewport(page, NOTIFICATION)) {
    throw new Error(
      `docsScreenshot(${filename}): a notification is in view and would keep its language; wait for it to close`,
    );
  }
  await capture(page, source.code, filename);

  const hasSelector = (await page.locator(LANGUAGE_SELECT).count()) > 0;
  if (!hasSelector && !options.reopenWaitingFor) {
    throw new Error(`docsScreenshot(${filename}): no language selector on this screen; pass reopenWaitingFor`);
  }
  const switchTo = async (code) => {
    await switchLanguage(page, code);
    if (options.afterSwitch) {
      await options.afterSwitch(page);
      await nextFrames(page);
    }
  };
  for (const language of others) {
    if (hasSelector) {
      await switchTo(language.code);
      await assertSubjectInView(page, filename, subject, language.code);
      await capture(page, language.code, filename);
    } else {
      await captureReopened(page, language, filename, subject, options.reopenWaitingFor);
    }
  }
  if (hasSelector) await switchTo(source.code);
}
