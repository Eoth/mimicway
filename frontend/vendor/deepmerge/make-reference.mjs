// Records in reference.json the answers of the original deepmerge 4.3.1 on generated cases (cases.mjs), the judgement
// deepmerge.test.js holds the stand-in to. Run by hand, from frontend/: `node vendor/deepmerge/make-reference.mjs`.
// It fetches the original with `npm pack` and checks it against the integrity npm published for it.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import fc from 'fast-check';
import { poisoned, run, VARIANTS } from './cases.mjs';

const ORIGINAL = 'deepmerge@4.3.1';
const INTEGRITY = 'sha512-3sUqbMEc77XqpdNO7FRyRog+eW3ph+GYCbj+rK+uYyRMuwsVy0rMiVtPn+QJlKFvWP/1PYpapqYn0Me2knFn+A==';
const SEED = 20261030;
const CASES = 600;

// Keys that collide between the values, most often `a` and `b` (`a` is the key of the customMerge variant), and those
// Object.prototype holds, through which the poisoning goes.
const key = fc.oneof(
  { weight: 3, arbitrary: fc.constantFrom('a', 'b') },
  { weight: 2, arbitrary: fc.constantFrom('c', '0', '@@s', '__proto__', 'constructor', 'toString') },
);
const leaf = fc.oneof(
  fc.integer({ min: -2, max: 2 }),
  fc.constantFrom('x', 'y'),
  fc.boolean(),
  fc.constant(null),
  fc.constantFrom({ $: 'undefined' }, { $: 'nan' }, { $: 'function' }, { $: 'map' }, { $: 'react' }),
  fc.record({ $: fc.constant('date'), v: fc.integer({ min: 0, max: 9 }) }),
  fc.record({ $: fc.constant('regexp'), v: fc.constantFrom('a+', '^b') }),
);
const { value, object } = fc.letrec((tie) => ({
  value: fc.oneof({ depthSize: 'small', withCrossShrink: true }, leaf, tie('array'), tie('object'), tie('instance')),
  array: fc.record({ $: fc.constant('array'), v: fc.array(tie('value'), { maxLength: 3 }) }),
  object: fc.record({ $: fc.constant('object'), o: fc.array(fc.tuple(key, tie('value')), { maxLength: 4 }) }),
  instance: fc.record({ $: fc.constant('instance'), o: fc.array(fc.tuple(key, tie('value')), { maxLength: 2 }) }),
}));
const testCase = fc.record({
  variant: fc.constantFrom(...Object.keys(VARIANTS)),
  // Mostly objects at the top, as configurations are, so that their keys meet.
  values: fc.array(fc.oneof({ weight: 4, arbitrary: object }, { weight: 1, arbitrary: value }), {
    minLength: 1,
    maxLength: 4,
  }),
});

function fetchOriginal(directory) {
  const [packed] = JSON.parse(
    execFileSync('npm', ['pack', ORIGINAL, '--json', '--pack-destination', directory], { encoding: 'utf8' }),
  );
  if (packed.integrity !== INTEGRITY) throw new Error(`${ORIGINAL}: integrity ${packed.integrity}, not ${INTEGRITY}`);
  execFileSync('tar', ['-xzf', path.join(directory, packed.filename), '-C', directory]);
  return createRequire(import.meta.url)(path.join(directory, 'package', 'dist', 'cjs.js'));
}

const directory = mkdtempSync(path.join(tmpdir(), 'deepmerge-reference-'));
try {
  const original = fetchOriginal(directory);
  // `poisoned` marks the cases where the original, given the `__proto__` keys, sets the prototype of what it returns.
  const cases = fc.sample(testCase, { seed: SEED, numRuns: CASES }).map((generated) => {
    const answer = run(original, generated, { withoutProto: true });
    return poisoned(run(original, generated)) ? { ...generated, answer, poisoned: true } : { ...generated, answer };
  });
  const here = path.dirname(fileURLToPath(import.meta.url));
  // One case per line, so that a new recording reads as a diff.
  const lines = cases.map((generated) => JSON.stringify(generated)).join(',\n');
  writeFileSync(path.join(here, 'reference.json'), `{"original":"${ORIGINAL}","cases":[\n${lines}\n]}\n`);
  const counted = cases.filter((generated) => generated.poisoned).length;
  console.log(`${cases.length} cases recorded from ${ORIGINAL}, ${counted} where it poisons the prototype.`);
} finally {
  rmSync(directory, { recursive: true, force: true });
}
