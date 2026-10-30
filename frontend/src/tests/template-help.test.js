// The template help of the builders. The catalogue it reads is rendered by the engine in a Rust test
// (src/engine/template/catalog_judge.rs): what these tests tie to the catalogue, the engine has judged.
import { describe, it, expect } from 'vitest';
import fc from 'fast-check';
import catalog from '../lib/template-catalog.json';
import {
  pipeOptions,
  fakeKinds,
  pipeProblems,
  pipeExample,
  expressionProblems,
  suggestAt,
  applySuggestion,
  splitPipes,
} from '../lib/template-help.js';
import { buildExpr, canPipeFixedValue } from '../lib/tpl-utils.js';

describe('the template catalogue', () => {
  it('labels every pipe and offers every fake data kind', () => {
    expect(pipeOptions.map((p) => p.name)).toEqual(catalog.pipes.map((p) => p.name));
    for (const p of pipeOptions) expect(p.label).toBeTruthy();
    expect(fakeKinds).toEqual(catalog.fake);
  });

  it('writes a fixed value and a pipe as the very template the engine rendered', () => {
    for (const p of catalog.pipes) {
      expect(buildExpr({ source: 'fixed', value: p.input, pipe: p.insert })).toBe(`{{"${p.input}" | ${p.insert}}}`);
    }
  });

  it('finds nothing wrong with any pipe it suggests, and each trap as the engine shows it', () => {
    for (const p of catalog.pipes) expect(pipeProblems(p.insert)).toEqual([]);
    for (const trap of catalog.traps) expect(pipeProblems(trap.pipe).map((x) => x.problem)).toEqual([trap.problem]);
  });

  it('shows the example of a single suggested pipe', () => {
    expect(pipeExample('first(3)')).toBe('“abcdef” → “abc”');
    expect(pipeExample('first(3) | upper')).toBeNull();
    expect(pipeExample('uper')).toBeNull();
  });
});

describe('pipe chains', () => {
  it('split at the bars outside parentheses, as the engine splits them', () => {
    expect(splitPipes(' first(9) | upper |  ')).toEqual(['first(9)', 'upper']);
    expect(splitPipes('replace("|","-") | lower')).toEqual(['replace("|","-")', 'lower']);
  });

  it('name each unknown pipe and each number argument that is not one', () => {
    expect(pipeProblems('upper | uper | first(N) | substr(2) | last( 4 )')).toEqual([
      { pipe: 'uper', name: 'uper', problem: 'unknown' },
      { pipe: 'first(N)', name: 'first', problem: 'number' },
    ]);
  });
});

describe('expressions of a free text', () => {
  it('are accepted when the engine knows their value and pipes', () => {
    for (const text of [
      'Hello {{query.name | upper}}!',
      '{{fake.Email}}',
      '{{"2026-01-01" | epoch_ms}}',
      'no expression',
    ]) {
      expect(expressionProblems(text)).toEqual([]);
    }
  });

  it('are reported when the value is unknown, a pipe is wrong or nothing closes them', () => {
    expect(expressionProblems('{{qery.id}}')).toHaveLength(1);
    expect(expressionProblems('{{fake.Nobody}}')).toHaveLength(1);
    expect(expressionProblems('{{query.id | uper}}')).toHaveLength(1);
    expect(expressionProblems('Hi {{query.id')).toHaveLength(1);
  });

  it('never break on any text', () => {
    fc.assert(
      fc.property(fc.string(), fc.nat(), (text, at) => {
        expect(Array.isArray(expressionProblems(text))).toBe(true);
        const caret = at % (text.length + 1);
        const s = suggestAt(text, caret);
        if (s) {
          expect(s.from).toBeLessThanOrEqual(caret);
          for (const item of s.items) {
            const next = applySuggestion(text, s, item);
            expect(next.text.slice(s.from, next.caret)).toBe(item.insert);
          }
        }
      }),
    );
  });
});

describe('suggestions in a free text', () => {
  const at = (text) => suggestAt(text, text.length);

  it('offer nothing outside an expression', () => {
    expect(at('Hello')).toBeNull();
    expect(at('{{uuid}} and')).toBeNull();
  });

  it('offer the values after {{, filtered by what is typed', () => {
    expect(at('Hi {{').items.map((i) => i.insert)).toContain('query.');
    expect(at('{{qu').items.map((i) => i.insert)).toEqual(['query.']);
    expect(at('{{fake.Ci').items.map((i) => i.insert)).toEqual(['fake.CityFR']);
  });

  it('offer the pipes after a bar', () => {
    expect(at('{{query.id | up').items.map((i) => i.insert)).toEqual(['upper']);
    expect(at('{{query.id | first(3) | ').items).toHaveLength(pipeOptions.length);
  });

  it('close the expression after a complete pick, never after a prefix', () => {
    const text = 'Id: {{uu';
    const s = at(text);
    const pick = applySuggestion(text, s, s.items[0]);
    expect(pick).toEqual({ text: 'Id: {{uuid}}', caret: 10 });
    const prefix = applySuggestion('{{qu', at('{{qu'), at('{{qu').items[0]);
    expect(prefix).toEqual({ text: '{{query.', caret: 8 });
    const closed = suggestAt('{{u}}', 3);
    expect(
      applySuggestion(
        '{{u}}',
        closed,
        closed.items.find((i) => i.insert === 'uuid'),
      ).text,
    ).toBe('{{uuid}}');
  });
});

describe('a fixed value with a pipe', () => {
  it('is written as quoted text only when the quotes hold it as it is', () => {
    expect(buildExpr({ source: 'fixed', value: 'abc', pipe: 'upper' })).toBe('{{"abc" | upper}}');
    expect(buildExpr({ source: 'fixed', value: 'a"b', pipe: 'upper' })).toBe('a"b');
    expect(buildExpr({ source: 'fixed', value: 'abc', pipe: '  ' })).toBe('abc');
    for (const c of ['"', '\\', '{', '}', '|', '(', ')', '&', '<']) expect(canPipeFixedValue(`a${c}b`)).toBe(false);
  });
});
