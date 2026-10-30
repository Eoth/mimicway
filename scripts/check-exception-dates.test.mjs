// Tests of check-exception-dates.mjs: every date from a year before to a year after a given day, judged by the day
// arithmetic of Date; each way of leaving an exception undated, unmatched or unreadable; and the files of the
// repository today. Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  checkExceptions,
  denyExceptions,
  osvExceptions,
  repositoryLists,
  HORIZON_DAYS,
} from './check-exception-dates.mjs';

const deny = (reason, id = 'RUSTSEC-2000-0001') => `[advisories]
version = 2
ignore = [
    # a comment
    { id = "${id}", reason = "${reason}" },
]

[licenses]
allow = ["MIT"]
`;
const osv = (date, id = 'RUSTSEC-2000-0001') => `# header
[[IgnoredVulns]]
id = "${id}"
${date ? `ignoreUntil = ${date}\n` : ''}reason = "why"
`;
const shift = (iso, days) => new Date(Date.parse(`${iso}T00:00:00Z`) + days * 86_400_000).toISOString().slice(0, 10);

test('a date passes only after today and within the horizon, in both files', () => {
  for (const today of ['2026-10-30', '2027-02-28', '2028-12-31']) {
    for (let days = -366; days <= 366; days++) {
      const date = shift(today, days);
      const { problems, count } = checkExceptions(
        deny(`no fix; review by ${date}`),
        { 'osv-scanner.toml': osv(date) },
        today,
      );
      const expected = days > 0 && days <= HORIZON_DAYS ? 0 : 2;
      assert.equal(problems.length, expected, `${date} seen on ${today}`);
      assert.equal(count, 2);
    }
  }
});

test('an exception with no date, or dated differently in the two files, fails', () => {
  const today = '2026-10-30';
  assert.equal(checkExceptions(deny('no fix'), { 'osv-scanner.toml': osv('2027-01-01') }, today).problems.length, 1);
  assert.equal(
    checkExceptions(deny('review by 2027-01-01'), { 'osv-scanner.toml': osv(null) }, today).problems.length,
    1,
  );
  assert.equal(
    checkExceptions(deny('review by 2027-01-01'), { 'osv-scanner.toml': osv('2027-01-02') }, today).problems.length,
    1,
  );
});

test('what deny.toml ignores, the root osv-scanner.toml ignores too; OSV may ignore more, in any directory', () => {
  const today = '2026-10-30';
  const elsewhere = { 'osv-scanner.toml': '', 'frontend/osv-scanner.toml': osv('2027-01-01') };
  const onlyDeny = checkExceptions(deny('review by 2027-01-01'), elsewhere, today);
  assert.match(onlyDeny.problems.join('\n'), /not by the root osv-scanner.toml/);
  const ghsa = { 'osv-scanner.toml': '', 'frontend/osv-scanner.toml': osv('2027-01-01', 'GHSA-xxxx-xxxx-xxxx') };
  const onlyOsv = checkExceptions('[advisories]\nversion = 2\n', ghsa, today);
  assert.deepEqual(onlyOsv.problems, []);
  assert.equal(onlyOsv.count, 1);
  const expired = { 'osv-scanner.toml': '', 'frontend/osv-scanner.toml': osv('2026-10-01', 'GHSA-xxxx-xxxx-xxxx') };
  assert.match(checkExceptions('[advisories]\n', expired, today).problems.join('\n'), /frontend.*expired/);
});

test('a list it cannot read whole is refused, never passed', () => {
  assert.throws(() => denyExceptions('[licenses]\n'), /no \[advisories\]/);
  assert.throws(() => denyExceptions('[advisories]\nignore = [\n    { reason = "x" },\n]\n'), /could not be read/);
  assert.throws(() => osvExceptions('[[IgnoredVulns]]\nreason = "x"\n'), /no id/);
  assert.deepEqual(denyExceptions('[advisories]\nignore = [\n    "RUSTSEC-2000-0002",\n]\n'), [
    { id: 'RUSTSEC-2000-0002', date: null },
  ]);
});

test('the exceptions of the repository are dated, today', () => {
  const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
  const today = new Date().toISOString().slice(0, 10);
  const [denyText, osvTexts] = repositoryLists(root);
  assert.ok('frontend/osv-scanner.toml' in osvTexts);
  const { problems, count } = checkExceptions(denyText, osvTexts, today);
  assert.deepEqual(problems, []);
  assert.ok(count > 0);
});
