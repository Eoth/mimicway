// Properties of path-params.js on generated URL patterns (fast-check): the parameter picker offers the names of the
// pattern's {name} and :name segments, in order, once each, and nothing else.
import { describe, it, expect } from 'vitest';
import fc from 'fast-check';
import { extractPathParamNames, combinePathParamNames } from '../lib/path-params.js';

const name = fc.stringMatching(/^[A-Za-z_][A-Za-z0-9_]{0,8}$/);
const segment = fc.oneof(
  name.map((n) => ({ text: `{${n}}`, name: n })),
  name.map((n) => ({ text: `:${n}`, name: n })),
  fc.stringMatching(/^[a-z0-9.-]{1,8}$/).map((text) => ({ text, name: null })),
  fc.constant({ text: '*', name: null }),
);
const distinct = (names) => [...new Set(names)];

describe('path-params properties', () => {
  it('a pattern gives the names of its parameter segments, in order of first appearance, once each', () => {
    fc.assert(
      fc.property(fc.array(segment, { maxLength: 8 }), fc.boolean(), (segments, trailingSlash) => {
        const pattern = `/${segments.map((s) => s.text).join('/')}${trailingSlash ? '/' : ''}`;
        expect(extractPathParamNames(pattern)).toEqual(distinct(segments.map((s) => s.name).filter(Boolean)));
      }),
    );
  });

  it('whatever the text, the names are distinct, not empty, and free of slashes', () => {
    fc.assert(
      fc.property(fc.string(), (pattern) => {
        const names = extractPathParamNames(pattern);
        expect(names).toEqual(distinct(names));
        for (const n of names) expect(n.length > 0 && !n.includes('/')).toBe(true);
      }),
    );
  });

  it('several patterns give the names of each, in order, once each', () => {
    fc.assert(
      fc.property(fc.array(fc.string(), { maxLength: 4 }), (patterns) => {
        expect(combinePathParamNames(patterns)).toEqual(distinct(patterns.flatMap(extractPathParamNames)));
      }),
    );
  });
});
