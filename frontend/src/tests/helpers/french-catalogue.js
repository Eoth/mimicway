// The test files that may load the French catalogue: french.test.js, which shows the interface in French, and
// l10n.test.js, which checks the catalogues themselves. Every other test runs in English (setup.js), so that rewording
// a French translation breaks these two files only.
export const MAY_LOAD_FRENCH = ['french.test.js', 'l10n.test.js'];

export const REFUSAL =
  `Only ${MAY_LOAD_FRENCH.join(' and ')} may load the French catalogue: assert the English text, ` +
  'or move a test about French into french.test.js.';

/** Whether the test file at `testPath` (absolute) may load the French catalogue. */
export function mayLoadFrench(testPath) {
  const path = String(testPath).replace(/\\/g, '/');
  return MAY_LOAD_FRENCH.some((name) => path.endsWith(`/src/tests/${name}`));
}
