import { describe, it, expect } from 'vitest';
import { formatDateTime, formatDateTimePrecise } from '../lib/format-date.js';

// Every screen formats its dates here, so `formatDateTime` must give exactly what `toLocaleString` gives for the same
// arguments.
describe('formatDateTime', () => {
  const ts = new Date('2026-01-10T12:05:09').getTime();

  it('without a locale or options: same as `toLocaleString()`', () => {
    expect(formatDateTime(ts)).toBe(new Date(ts).toLocaleString());
  });

  it('with a locale and no options: same as `toLocaleString(locale)`', () => {
    expect(formatDateTime(ts, undefined, 'fr-FR')).toBe(new Date(ts).toLocaleString('fr-FR'));
  });

  it('with a locale and options: same as `toLocaleString(locale, options)`', () => {
    const options = { year: 'numeric', month: 'long' };
    expect(formatDateTime(ts, options, 'en-US')).toBe(new Date(ts).toLocaleString('en-US', options));
  });
});

describe('formatDateTimePrecise', () => {
  const ts = new Date('2026-01-10T12:05:09').getTime();

  // Outside French, the interface has no regional form of its own: the logs follow the browser's locale, en-US in
  // these tests (setup.js). The French form is checked in context by french.test.js.
  it('writes every part with two digits, in the browser locale when the interface is in English', () => {
    expect(formatDateTimePrecise(ts)).toMatch(/^01\/10\/26, 12:05:09\sPM$/);
  });

  it('takes an explicit locale', () => {
    expect(formatDateTimePrecise(ts, 'fr-FR')).toBe('10/01/26 12:05:09');
  });
});
