// The stand-in judged by the original deepmerge 4.3.1: on every case of reference.json it gives the answer of the
// original on the same values without their `__proto__` keys (same values, same objects shared with the inputs, same
// errors), and so never the prototype the original sets from such a key.
//
// What it does not see: inputs the generator of make-reference.mjs never builds (getters, non-enumerable properties,
// cycles, objects of another realm), and a change of behaviour in a later release of the original.

import { readFileSync, realpathSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { build, encode, poisoned, run, VARIANTS } from './cases.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const deepmerge = require('./index.js');
const { original, cases } = JSON.parse(readFileSync(path.join(here, 'reference.json'), 'utf8'));

describe(`deepmerge stand-in judged by ${original}`, () => {
  it('is the deepmerge the dependency tree resolves to', () => {
    const fromTree = createRequire(path.join(here, '..', '..', 'package.json')).resolve('deepmerge');
    expect(realpathSync(fromTree)).toBe(path.join(here, 'index.js'));
  });

  it('holds, for every calling convention of cases.mjs, cases where the original answers a value', () => {
    for (const variant of Object.keys(VARIANTS)) {
      const answered = cases.filter((testCase) => testCase.variant === variant && testCase.answer?.$ !== 'throws');
      expect(answered.length, variant).toBeGreaterThan(variant === 'notAnArray' ? 0 : 30);
    }
  });

  it('gives the answer of the original without the __proto__ keys, on every case', () => {
    expect(cases.length).toBeGreaterThan(500);
    for (const testCase of cases) expect(run(deepmerge, testCase), JSON.stringify(testCase)).toEqual(testCase.answer);
  });

  it('meets cases where the original poisons, and leaves the inputs untouched', () => {
    const attacks = cases.filter((testCase) => testCase.poisoned);
    expect(attacks.length).toBeGreaterThan(10);
    for (const testCase of attacks) expect(poisoned(run(deepmerge, testCase)), JSON.stringify(testCase)).toBe(false);
    for (const testCase of cases) {
      const values = testCase.values.map((description) => build(description));
      const before = encode(values);
      try {
        VARIANTS[testCase.variant](deepmerge, values);
      } catch {
        // The answer, error included, is judged above; only the inputs matter here.
      }
      expect(encode(values), JSON.stringify(testCase)).toEqual(before);
    }
  });

  it('drops a parsed __proto__ key and leaves Object.prototype alone', () => {
    const merged = deepmerge({ a: 1 }, { a: JSON.parse('{"__proto__": {"polluted": true}, "kept": 1}') });
    expect(Object.getPrototypeOf(merged.a)).toBe(Object.prototype);
    expect(merged.a).toEqual({ kept: 1 });
    expect(Object.prototype.hasOwnProperty.call(merged.a, '__proto__')).toBe(false);
    expect({}.polluted).toBeUndefined();
  });
});
