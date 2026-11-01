// The accessibility violations the suite accepts for a while (a11y-exceptions.json), and the check of their dates that
// runs before any test (global-setup.mjs). Kept apart from fixtures.mjs, which loads Playwright, so that the
// repository checks test it without the interface's dependencies installed.
import { readFileSync } from 'node:fs';

export const HORIZON_DAYS = 183;

/** `{ rule, screen, target, until, reason }`: a known violation, accepted on that view and element until `until`. */
export const EXCEPTIONS = JSON.parse(readFileSync(new URL('./a11y-exceptions.json', import.meta.url), 'utf8'));

/** What is wrong with the exceptions on `today` (YYYY-MM-DD): a date passed, missing or too far, a field missing. */
export function exceptionProblems(exceptions, today) {
  const day = (date) => Date.parse(`${date}T00:00:00Z`) / 86_400_000;
  return exceptions.flatMap((e, i) => {
    const name = `a11y-exceptions.json #${i + 1} (${e.rule ?? 'no rule'})`;
    if (!e.rule || !e.screen || !e.target || !e.reason)
      return [`${name}: rule, screen, target and reason are required`];
    if (!/^\d{4}-\d{2}-\d{2}$/.test(e.until ?? '') || Number.isNaN(day(e.until)))
      return [`${name}: no valid until date`];
    if (day(e.until) <= day(today)) return [`${name}: its date ${e.until} has passed`];
    if (day(e.until) > day(today) + HORIZON_DAYS)
      return [`${name}: ${e.until} is more than ${HORIZON_DAYS} days ahead`];
    return [];
  });
}
