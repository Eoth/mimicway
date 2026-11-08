// Tests of scale-test.mjs: the verdict of a family from its three counts (red when it grows faster than its class
// allows, red when it barely grows, red when nothing was counted), the growth each class allows, and the reading of
// Callgrind's output and of the probe's list of families. Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { TOLERANCE, classRatio, judge, parseFamilies, parseTotals } from './scale-test.mjs';

// Counts of a cost made of a fixed part, a linear part and a quadratic part, at n, 2n and 4n.
const counts = (n, { fixed = 0, linear = 0, quadratic = 0 }) =>
  [n, 2 * n, 4 * n].map((m) => fixed + linear * m + quadratic * m * m);

test('a linear cost passes, with or without a modest fixed part', () => {
  assert.equal(judge({ growth: 'linear', n: 200, counts: counts(200, { linear: 5000 }) }).ok, true);
  assert.equal(
    judge({
      growth: 'linear',
      n: 200,
      counts: counts(200, { fixed: 100_000, linear: 5000 }),
    }).ok,
    true,
  );
});

test('a quadratic cost fails a linear family, even when it is a small share of the count at n', () => {
  const verdict = judge({
    growth: 'linear',
    n: 200,
    counts: counts(200, { quadratic: 10 }),
  });
  assert.equal(verdict.ok, false);
  assert.match(verdict.reason, /more than linear allows/);
  // A tenth of the count at n is enough.
  const share = judge({
    growth: 'linear',
    n: 100,
    counts: counts(100, { linear: 9000, quadratic: 10 }),
  });
  assert.equal(share.ok, false);
});

test('a quadratic cost fails an n log n family, which an n log n cost passes', () => {
  const nlogn = [100, 200, 400].map((m) => 1000 * m * Math.log2(m));
  assert.equal(judge({ growth: 'n log n', n: 100, counts: nlogn }).ok, true);
  assert.equal(judge({ growth: 'n log n', n: 100, counts: counts(100, { quadratic: 10 }) }).ok, false);
});

test('a family whose fixed cost hides its growth fails, and says to raise n', () => {
  const verdict = judge({
    growth: 'linear',
    n: 10,
    counts: counts(10, { fixed: 1_000_000, linear: 100 }),
  });
  assert.equal(verdict.ok, false);
  assert.match(verdict.reason, /raise n/);
});

test('a family where nothing was counted fails', () => {
  assert.equal(judge({ growth: 'linear', n: 10, counts: [0, 0, 0] }).ok, false);
});

test('each class allows its growth from n to 4n, times the tolerance', () => {
  assert.equal(classRatio('linear', 200), 4);
  assert.ok(Math.abs(classRatio('n log n', 256) - 4 * (10 / 8)) < 1e-9);
  assert.throws(() => classRatio('quadratic', 10), /unknown growth class/);
  assert.ok(TOLERANCE > 1 && TOLERANCE < 2, 'a quadratic step must stay out of reach of a linear family');
});

test('the totals of a Callgrind output are read', () => {
  const output = 'version: 1\ncreator: callgrind-3.26.0\nevents: Ir\nsummary: 1234\ntotals: 1234\n';
  assert.equal(parseTotals(output), 1234);
  assert.equal(parseTotals('events: Ir\nsummary: 99\n'), 99);
  assert.throws(() => parseTotals('events: Ir\n'), /no totals/);
});

test('the families listed by the probe are read', () => {
  const families = parseFamilies('rules\t200\tlinear\tone request\nscript\t60\tn log n\tvalues\n');
  assert.deepEqual(families, [
    { name: 'rules', n: 200, growth: 'linear', about: 'one request' },
    { name: 'script', n: 60, growth: 'n log n', about: 'values' },
  ]);
});
