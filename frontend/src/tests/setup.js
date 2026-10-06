import '@testing-library/jest-dom/vitest';
import { expect, vi } from 'vitest';
import { setLocale } from '../lib/i18n.svelte.js';

// The tests run as an en-US browser in UTC would, whatever the machine (machine-locale.test.js). Without a locale,
// the interface formats dates in the browser's (intlLocale()), which Node takes from the machine: fr-FR on a French
// developer's machine, en-US on a CI runner, so a test could pass on one and fail on the other.
process.env.TZ = 'UTC';
const BROWSER_LOCALE = 'en-US';
for (const [type, methods] of [
  [Date, ['toLocaleString', 'toLocaleDateString', 'toLocaleTimeString']],
  [Number, ['toLocaleString']],
]) {
  for (const method of methods) {
    const original = type.prototype[method];
    if (original.machineLocale) continue;
    const pinned = function (locales, options) {
      return original.call(this, locales ?? BROWSER_LOCALE, options);
    };
    pinned.machineLocale = original;
    type.prototype[method] = pinned;
  }
}

// The unit tests run in English, the language of the code: they assert the texts written where they are used, and
// rewording a translation breaks none of them. french.test.js shows the interface in French; l10n.test.js checks the
// catalogues as a whole.
await setLocale('en');

// Any other test file that loads the French catalogue fails on its first use: switching to French there would assert
// French texts again. The helper is imported here, not at the top, because vi.mock runs before the imports.
vi.mock('../locales/fr.json', async (importOriginal) => {
  const { mayLoadFrench, REFUSAL } = await import('./helpers/french-catalogue.js');
  if (mayLoadFrench(expect.getState().testPath)) return importOriginal();
  return {
    get default() {
      throw new Error(REFUSAL);
    },
  };
});
