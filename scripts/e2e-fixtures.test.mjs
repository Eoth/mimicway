// Tests of the accessibility check of the end-to-end suite (frontend/e2e/fixtures.mjs): every spec of the directory,
// found by reading it, takes `test` from the fixture, so that no screen escapes it; and the dates of its exceptions
// are judged against the day arithmetic of Date over two years. Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { EXCEPTIONS, exceptionProblems, HORIZON_DAYS } from '../frontend/e2e/a11y-exceptions.mjs';

const e2e = new URL('../frontend/e2e/', import.meta.url);
const specs = readdirSync(e2e).filter((name) => /\.spec\.m?js$/.test(name));

test('every spec takes test from the accessibility fixture, never from Playwright', () => {
  assert.ok(specs.length > 20, `only ${specs.length} specs found`);
  for (const name of specs) {
    const source = readFileSync(new URL(name, e2e), 'utf8');
    assert.match(source, /import \{[^}]*\btest\b[^}]*\} from '\.\/fixtures\.mjs'/, name);
    assert.doesNotMatch(source, /from '@playwright\/test'/, name);
  }
});

test('an exception is valid exactly from the day after today to the horizon', () => {
  const today = '2026-11-01';
  const base = Date.parse(`${today}T00:00:00Z`);
  for (let offset = -400; offset <= 400; offset += 1) {
    const until = new Date(base + offset * 86_400_000).toISOString().slice(0, 10);
    const problems = exceptionProblems([{ rule: 'r', screen: 's', target: 't', reason: 'why', until }], today);
    const valid = offset >= 1 && offset <= HORIZON_DAYS;
    assert.equal(problems.length === 0, valid, `${until}: ${problems}`);
  }
});

test('an exception without a field or a readable date is refused', () => {
  const ok = { rule: 'r', screen: 's', target: 't', reason: 'why', until: '2026-12-01' };
  for (const field of Object.keys(ok)) {
    const { [field]: _, ...missing } = ok;
    assert.equal(exceptionProblems([missing], '2026-11-01').length, 1, field);
  }
  for (const until of ['2026-13-01', '1 Dec 2026', '2026-12-1']) {
    assert.equal(exceptionProblems([{ ...ok, until }], '2026-11-01').length, 1, until);
  }
});

test('the exceptions of the repository are valid today', () => {
  assert.deepEqual(exceptionProblems(EXCEPTIONS, new Date().toISOString().slice(0, 10)), []);
});
