import { describe, expect, it } from 'vitest';
import fc from 'fast-check';
import { highlightRhai } from '../lib/rhai-highlight.js';
import { RHAI_KEYWORDS } from '../lib/rhai-functions.js';

const kinds = (text) => highlightRhai(text).map(({ text, kind }) => [text, kind]);

describe('highlightRhai', () => {
  // The colored copy lies under the text area: a character lost or added would shift every color after it.
  it('gives back the text exactly, whatever it holds', () => {
    const piece = fc.constantFrom(
      'let',
      'fn',
      'x1',
      ' ',
      '\n',
      '(',
      ')',
      '.',
      '"',
      "'",
      '`',
      '\\',
      '/',
      '*',
      '//',
      '/*',
      '*/',
      '${',
      '}',
      '0x1f',
      '1.5',
      'é',
      '🙂',
    );
    fc.assert(
      fc.property(
        fc.oneof(
          fc.string({ unit: 'binary' }),
          fc.array(piece).map((p) => p.join('')),
        ),
        (text) => {
          const pieces = highlightRhai(text);
          expect(pieces.map((p) => p.text).join('')).toBe(text);
          expect(pieces.every((p) => p.text.length > 0)).toBe(true);
        },
      ),
      { numRuns: 2000 },
    );
  });

  it('colors every keyword of the completion as a keyword', () => {
    for (const { name } of RHAI_KEYWORDS) expect(kinds(name), name).toEqual([[name, 'keyword']]);
  });

  it('tells apart each kind of piece of its rule', () => {
    expect(kinds('let total = sum(items) + 1_000;')).toEqual([
      ['let', 'keyword'],
      [' total = ', null],
      ['sum', 'function'],
      ['(items) + ', null],
      ['1_000', 'number'],
      [';', null],
    ]);
    expect(kinds('request.body.len ()')).toEqual([
      ['request.body.', null],
      ['len', 'function'],
      [' ()', null],
    ]);
    expect(kinds('0xff 1.5e3 x.0')).toEqual([
      ['0xff', 'number'],
      [' ', null],
      ['1.5e3', 'number'],
      [' x.', null],
      ['0', 'number'],
    ]);
    expect(kinds('try { } catch (e) { } do { } until x; this')).toEqual([
      ['try', 'keyword'],
      [' { } ', null],
      ['catch', 'keyword'],
      [' (e) { } ', null],
      ['do', 'keyword'],
      [' { } ', null],
      ['until', 'keyword'],
      [' x; ', null],
      ['this', 'keyword'],
    ]);
  });

  it('keeps strings and comments whole, with what looks like code inside', () => {
    expect(kinds('"a \\" let f(" + x')).toEqual([
      ['"a \\" let f("', 'string'],
      [' + x', null],
    ]);
    expect(kinds('"open\nlet')).toEqual([
      ['"open', 'string'],
      ['\n', null],
      ['let', 'keyword'],
    ]);
    expect(kinds('`a\n${f(x)}` 1')).toEqual([
      ['`a\n${f(x)}`', 'string'],
      [' ', null],
      ['1', 'number'],
    ]);
    expect(kinds('x // let f()\nlet')).toEqual([
      ['x ', null],
      ['// let f()', 'comment'],
      ['\n', null],
      ['let', 'keyword'],
    ]);
    expect(kinds('/* a /* b */ c */ let')).toEqual([
      ['/* a /* b */ c */', 'comment'],
      [' ', null],
      ['let', 'keyword'],
    ]);
    expect(kinds('/* never closed\nlet')).toEqual([['/* never closed\nlet', 'comment']]);
  });
});
