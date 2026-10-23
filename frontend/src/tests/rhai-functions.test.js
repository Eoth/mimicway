import { describe, it, expect } from 'vitest';
import {
  RHAI_FUNCTIONS,
  RHAI_KEYWORDS,
  filterRhaiFunctions,
  filterRhaiSuggestions,
  tokenAtCursor,
  computeInsertSelection,
} from '../lib/rhai-functions.js';

describe('RHAI_FUNCTIONS (single source)', () => {
  it('every function has a name, a signature, a description and an insertText', () => {
    for (const fn of RHAI_FUNCTIONS) {
      expect(fn.name).toBeTruthy();
      expect(fn.signature).toContain(fn.name);
      expect(fn.description.length).toBeGreaterThan(0);
      expect(fn.insertText).toContain(fn.name);
    }
  });

  it('function names are unique', () => {
    const names = RHAI_FUNCTIONS.map((f) => f.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it('includes the seeded functions and the date functions', () => {
    const names = RHAI_FUNCTIONS.map((f) => f.name);
    expect(names).toContain('seeded_int');
    expect(names).toContain('seeded_pick');
    expect(names).toContain('date_now');
    expect(names).toContain('date_past');
    expect(names).toContain('date_future');
  });

  it('includes parse_date, whose description mentions milliseconds and the error case', () => {
    const parseDate = RHAI_FUNCTIONS.find((f) => f.name === 'parse_date');
    expect(parseDate).toBeTruthy();
    expect(parseDate.signature).toBe('parse_date(text, "pattern")');
    expect(parseDate.description).toContain('milliseconds');
    expect(parseDate.description.toLowerCase()).toContain('error');
  });

  it('includes the 4 request accessors (path/query/headers/body)', () => {
    const names = RHAI_FUNCTIONS.map((f) => f.name);
    expect(names).toContain('request.path');
    expect(names).toContain('request.query');
    expect(names).toContain('request.headers');
    expect(names).toContain('request.body');
  });

  it('says that header names are lowercased', () => {
    const headers = RHAI_FUNCTIONS.find((f) => f.name === 'request.headers');
    expect(headers.description.toLowerCase()).toContain('lowercase');
  });
});

describe('filterRhaiFunctions', () => {
  it('returns the whole list for an empty query', () => {
    expect(filterRhaiFunctions('')).toEqual(RHAI_FUNCTIONS);
    expect(filterRhaiFunctions(undefined)).toEqual(RHAI_FUNCTIONS);
  });

  it('filters by prefix, ignoring case', () => {
    const matches = filterRhaiFunctions('SEED');
    expect(matches.map((f) => f.name)).toEqual(['seeded_int', 'seeded_pick']);
  });

  it('returns nothing for an unknown prefix', () => {
    expect(filterRhaiFunctions('zzz')).toEqual([]);
  });

  it('matches the prefix only, not any substring', () => {
    // `date_now` contains "now" but does not start with it.
    const matches = filterRhaiFunctions('now');
    expect(matches.map((f) => f.name).sort()).toEqual(['now_iso', 'now_ms']);
  });

  it('typing "request" offers the 4 request accessors (path/query/headers/body)', () => {
    const matches = filterRhaiFunctions('request');
    expect(matches.map((f) => f.name).sort()).toEqual([
      'request.body',
      'request.headers',
      'request.path',
      'request.query',
    ]);
  });
});

describe('filterRhaiSuggestions', () => {
  it('lists the keywords before the functions', () => {
    expect(filterRhaiSuggestions('').map((s) => s.name)).toEqual([
      ...RHAI_KEYWORDS.map((k) => k.name),
      ...RHAI_FUNCTIONS.map((f) => f.name),
    ]);
    expect(filterRhaiSuggestions('re').map((s) => s.name)).toEqual([
      'return',
      'request.path',
      'request.query',
      'request.headers',
      'request.body',
    ]);
  });

  it('offers no keyword that the sandbox refuses', () => {
    const names = RHAI_KEYWORDS.map((k) => k.name);
    expect(names).not.toContain('eval');
    expect(names).not.toContain('import');
  });

  it('gives every keyword a placeholder or a caret that its skeleton holds', () => {
    for (const k of RHAI_KEYWORDS) {
      if (k.select) expect(k.insertText, k.name).toContain(k.select);
      if (k.caretAfter) expect(k.insertText, k.name).toContain(k.caretAfter);
    }
  });
});

describe('tokenAtCursor', () => {
  it('extracts the Rhai identifier right before the caret', () => {
    expect(tokenAtCursor('seed', 4)).toEqual({ token: 'seed', start: 0 });
  });

  it('returns an empty token right after a space or a parenthesis', () => {
    expect(tokenAtCursor('foo(', 4)).toEqual({ token: '', start: 4 });
    expect(tokenAtCursor('foo ', 4)).toEqual({ token: '', start: 4 });
  });

  it('ignores the text after the caret', () => {
    expect(tokenAtCursor('rand_int', 4)).toEqual({ token: 'rand', start: 0 });
  });

  it('works on the last line of a multi-line text', () => {
    const text = 'let x = 1;\nseeded_pi';
    expect(tokenAtCursor(text, text.length)).toEqual({ token: 'seeded_pi', start: 11 });
  });
});

describe('computeInsertSelection', () => {
  it('selects the arguments between the parentheses', () => {
    expect(computeInsertSelection('random_int(min, max)')).toEqual({ start: 11, end: 19 });
  });

  it('puts the caret at the end for a call without arguments', () => {
    const sel = computeInsertSelection('now_ms()');
    expect(sel).toEqual({ start: 8, end: 8 });
  });

  it("selects a keyword's placeholder, or puts the caret where its block starts", () => {
    expect(computeInsertSelection('let name = value;', { select: 'name' })).toEqual({ start: 4, end: 8 });
    expect(computeInsertSelection('else {\n    \n}', { caretAfter: '{\n    ' })).toEqual({ start: 11, end: 11 });
  });

  it('puts the caret at the end for a request accessor (no parentheses)', () => {
    const sel = computeInsertSelection('request.path');
    expect(sel).toEqual({ start: 'request.path'.length, end: 'request.path'.length });
  });
});
