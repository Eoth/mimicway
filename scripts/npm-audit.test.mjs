// Tests of npm-audit.mjs: reports drawn at random (advisories of every severity, packages vulnerable through
// others, exceptions dated before, on and after the day), judged by a rule written here apart from the script; a
// report in the shape npm 11 gives, reduced from a real one; and every way a report is not one.
// Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { AUDIT, judgeAudit } from './npm-audit.mjs';

const TODAY = '2026-11-17';
const ghsa = (n) => `GHSA-${String(n).padStart(4, '0')}-aaaa-bbbb`;
const advisory = (id, severity, name) => ({
  source: 1,
  name,
  dependency: name,
  title: `a defect of ${name}`,
  url: `https://github.com/advisories/${id}`,
  severity,
  range: '<=1.0.0',
});
const RANK = ['info', 'low', 'moderate', 'high', 'critical'];
const worst = (severities) => RANK[Math.max(...severities.map((s) => RANK.indexOf(s)))];

/** A report as npm prints it, from packages `{ name, advisories: [{ id, severity }], through: [names] }`. */
function report(packages) {
  const severityOf = (pkg, seen = new Set()) => {
    if (seen.has(pkg.name)) return 'info';
    seen.add(pkg.name);
    const through = pkg.through.map((name) =>
      severityOf(
        packages.find((p) => p.name === name),
        seen,
      ),
    );
    return worst([...pkg.advisories.map((a) => a.severity), ...through]);
  };
  const vulnerabilities = {};
  const counts = {
    info: 0,
    low: 0,
    moderate: 0,
    high: 0,
    critical: 0,
    total: packages.length,
  };
  for (const pkg of packages) {
    const severity = severityOf(pkg);
    counts[severity]++;
    vulnerabilities[pkg.name] = {
      name: pkg.name,
      severity,
      isDirect: false,
      via: [...pkg.advisories.map((a) => advisory(a.id, a.severity, pkg.name)), ...pkg.through],
      effects: [],
      range: '*',
      nodes: [`node_modules/${pkg.name}`],
      fixAvailable: false,
    };
  }
  return {
    auditReportVersion: 2,
    vulnerabilities,
    metadata: { vulnerabilities: counts },
  };
}

// A small generator with a seed, so that a failure names the draw that gave it.
function random(seed) {
  let state = seed >>> 0;
  return (bound) => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state % bound;
  };
}

test('drawn reports: the audit fails exactly on the high and critical advisories no exception of the day covers', () => {
  const dates = ['2026-11-16', '2026-11-17', '2026-11-18', '2027-03-01'];
  let failed = 0;
  let passed = 0;
  for (let seed = 1; seed <= 3000; seed++) {
    const draw = random(seed);
    const packages = [];
    const leaves = 1 + draw(4);
    for (let i = 0; i < leaves; i++) {
      const advisories = [];
      for (let k = 1 + draw(2); k > 0; k--) advisories.push({ id: ghsa(draw(6)), severity: RANK[draw(5)] });
      packages.push({ name: `leaf${i}`, advisories, through: [] });
    }
    for (let i = draw(4); i > 0; i--) {
      const through = [packages[draw(packages.length)].name];
      packages.push({ name: `tool${i}`, advisories: [], through });
    }
    const exceptions = [];
    for (let i = draw(4); i > 0; i--) {
      const id = ghsa(draw(6));
      if (!exceptions.some((e) => e.id === id)) exceptions.push({ id, date: dates[draw(dates.length)] });
    }

    // The rule, written apart: an exception, listed once, counts when it is dated after the day.
    const covered = new Set(exceptions.filter((e) => e.date > TODAY).map((e) => e.id));
    const expected = new Set();
    for (const pkg of packages) {
      for (const a of pkg.advisories) {
        if ((a.severity === 'high' || a.severity === 'critical') && !covered.has(a.id)) expected.add(a.id);
      }
    }
    const verdict = judgeAudit(report(packages), exceptions, TODAY);
    const named = new Set(verdict.problems.map((p) => p.match(/GHSA(?:-[0-9a-z]{4}){3}/)?.[0]));
    assert.deepEqual([...named].sort(), [...expected].sort(), `seed ${seed}`);
    assert.equal(verdict.problems.length, expected.size, `seed ${seed}`);
    if (expected.size > 0) failed++;
    else passed++;
  }
  // Both verdicts were drawn often: a generator that only gave one would judge nothing.
  assert.ok(failed > 300 && passed > 300, `${failed} failed, ${passed} passed`);
});

// The report of 2026-11-17 for the interface, reduced: one advisory, and five packages vulnerable through it.
const REAL = report([
  {
    name: 'picomatch',
    advisories: [{ id: 'GHSA-fc5q-39xc-vjpf', severity: 'high' }],
    through: [],
  },
  { name: 'fdir', advisories: [], through: ['picomatch'] },
  { name: 'tinyglobby', advisories: [], through: ['fdir', 'picomatch'] },
  { name: 'vite', advisories: [], through: ['picomatch', 'tinyglobby'] },
  {
    name: 'vitest',
    advisories: [],
    through: ['picomatch', 'tinyglobby', 'vite'],
  },
  { name: '@sveltejs/vite-plugin-svelte', advisories: [], through: ['vite'] },
]);
const PICOMATCH = { id: 'GHSA-fc5q-39xc-vjpf', date: '2026-12-17' };

test('one advisory and the packages it reaches: red without its exception, green with it, red again on its date', () => {
  assert.equal(REAL.metadata.vulnerabilities.high, 6);
  const bare = judgeAudit(REAL, [], TODAY);
  assert.equal(bare.problems.length, 1);
  assert.match(bare.problems[0], /^GHSA-fc5q-39xc-vjpf \(high, picomatch: /);

  const excepted = judgeAudit(REAL, [PICOMATCH], TODAY);
  assert.deepEqual(excepted.problems, []);
  assert.equal(excepted.excepted.length, 1);
  assert.match(excepted.excepted[0], /ignored until 2026-12-17$/);

  assert.deepEqual(judgeAudit(REAL, [PICOMATCH], '2026-12-16').problems, []);
  assert.equal(judgeAudit(REAL, [PICOMATCH], '2026-12-17').problems.length, 1);
  assert.equal(judgeAudit(REAL, [PICOMATCH], '2027-01-01').problems.length, 1);
});

test('an exception covers its advisory only: another high one fails, a moderate one does not', () => {
  const withAnother = (severity) =>
    report([
      {
        name: 'picomatch',
        advisories: [{ id: 'GHSA-fc5q-39xc-vjpf', severity: 'high' }],
        through: [],
      },
      { name: 'other', advisories: [{ id: ghsa(1), severity }], through: [] },
    ]);
  for (const severity of ['high', 'critical']) {
    const verdict = judgeAudit(withAnother(severity), [PICOMATCH], TODAY);
    assert.equal(verdict.problems.length, 1, severity);
    assert.match(verdict.problems[0], new RegExp(`^${ghsa(1)} \\(${severity}, other: `));
  }
  for (const severity of ['info', 'low', 'moderate']) {
    assert.deepEqual(judgeAudit(withAnother(severity), [PICOMATCH], TODAY).problems, [], severity);
  }
  // Two advisories of one package: the exception of the first leaves the second.
  const twice = report([
    {
      name: 'picomatch',
      advisories: [
        { id: 'GHSA-fc5q-39xc-vjpf', severity: 'high' },
        { id: ghsa(2), severity: 'high' },
      ],
      through: [],
    },
  ]);
  assert.equal(judgeAudit(twice, [PICOMATCH], TODAY).problems.length, 1);
});

test('nothing vulnerable passes, and exceptions that cover nothing change nothing', () => {
  assert.deepEqual(judgeAudit(report([]), [], TODAY), {
    problems: [],
    failing: [],
    excepted: [],
  });
  assert.deepEqual(judgeAudit(report([]), [PICOMATCH], TODAY).problems, []);
});

test('a high advisory that no exception could name fails', () => {
  const nameless = report([{ name: 'p', advisories: [{ id: ghsa(1), severity: 'high' }], through: [] }]);
  // The identifier is read from the address of the advisory's own page, whole, and from nowhere else.
  const elsewhere = [
    'https://example.com/elsewhere',
    `https://example.com/advisories/${ghsa(1)}`,
    `https://github.com/advisories/${ghsa(1)}/x`,
    `https://github.com/advisories/x/${ghsa(1)}`,
    `https://github.com/other/${ghsa(1)}`,
    `http://github.com/advisories/${ghsa(1)}`,
    `https://github.com/advisories/${ghsa(1)}?x`,
    `x https://github.com/advisories/${ghsa(1)}`,
    ghsa(1),
  ];
  for (const url of elsewhere) {
    nameless.vulnerabilities.p.via[0].url = url;
    assert.equal(judgeAudit(nameless, [{ id: ghsa(1), date: '2027-01-01' }], TODAY).problems.length, 1, url);
  }
  nameless.vulnerabilities.p.via[0].url = `https://github.com/advisories/${ghsa(1)}`;
  assert.deepEqual(judgeAudit(nameless, [{ id: ghsa(1), date: '2027-01-01' }], TODAY).problems, []);
  delete nameless.vulnerabilities.p.via[0].url;
  assert.equal(judgeAudit(nameless, [{ id: ghsa(1), date: '2027-01-01' }], TODAY).problems.length, 1);
});

test('a package said high that no advisory of the report explains fails', () => {
  const orphan = report([{ name: 'p', advisories: [{ id: ghsa(1), severity: 'high' }], through: [] }]);
  orphan.vulnerabilities.p.via = ['gone'];
  assert.match(judgeAudit(orphan, [], TODAY).problems[0], /^p is said high and no advisory/);
  // Two packages that only name each other explain nothing either.
  const circle = report([
    {
      name: 'a',
      advisories: [{ id: ghsa(1), severity: 'critical' }],
      through: ['b'],
    },
    { name: 'b', advisories: [], through: ['a'] },
  ]);
  circle.vulnerabilities.a.via = ['b'];
  assert.equal(judgeAudit(circle, [], TODAY).problems.length, 2);
});

test('what is not a whole report is refused, never passed', () => {
  const good = () =>
    report([
      {
        name: 'p',
        advisories: [{ id: ghsa(1), severity: 'low' }],
        through: [],
      },
    ]);
  const broken = [
    null,
    'text',
    {},
    { error: { code: 'ENOLOCK', summary: 'no lock file' } },
    { ...good(), error: 'x' },
    { vulnerabilities: {} },
    { vulnerabilities: [], metadata: { vulnerabilities: {} } },
    { metadata: good().metadata },
    (() => {
      const r = good();
      r.metadata.vulnerabilities.high = 1;
      return r;
    })(),
    (() => {
      const r = good();
      delete r.vulnerabilities.p;
      return r;
    })(),
    (() => {
      const r = good();
      r.vulnerabilities.p.severity = 'severe';
      return r;
    })(),
    (() => {
      const r = good();
      r.vulnerabilities.p.via = 'p';
      return r;
    })(),
    (() => {
      const r = good();
      r.vulnerabilities.p.via = [null];
      return r;
    })(),
    (() => {
      const r = good();
      r.vulnerabilities.p.via[0].severity = undefined;
      return r;
    })(),
  ];
  broken.forEach((r, i) => assert.throws(() => judgeAudit(r, [], TODAY), Error, `report ${i}`));
  assert.deepEqual(judgeAudit(good(), [], TODAY).problems, []);
  // An exception without a date is one that would never end.
  assert.throws(() => judgeAudit(good(), [{ id: ghsa(1), date: null }], TODAY), /no ignoreUntil/);
});

test('a package said more severe than every advisory it leads to fails, even with the exception of those', () => {
  const report = structuredClone(REAL);
  report.vulnerabilities.vite.severity = 'critical';
  report.metadata.vulnerabilities.high = 5;
  report.metadata.vulnerabilities.critical = 1;
  const verdict = judgeAudit(report, [PICOMATCH], TODAY);
  assert.deepEqual(verdict.problems, ['vite is said critical and no advisory of the report explains it']);
  // The same package as severe as the advisory it leads to is explained by it.
  assert.deepEqual(judgeAudit(REAL, [PICOMATCH], TODAY).problems, []);
});

test('a list of exceptions that cannot be trusted is refused: an identifier twice, a date that is not a day', () => {
  const twice = [PICOMATCH, { ...PICOMATCH, date: '2026-11-01' }];
  assert.throws(() => judgeAudit(REAL, twice, TODAY), /listed twice/);
  assert.throws(() => judgeAudit(REAL, [...twice].reverse(), TODAY), /listed twice/);
  for (const date of ['2026-13-40', '2026-02-30', '2026-12-17T00:00', '26-12-17', '9999', ' 2026-12-17', 20261217]) {
    assert.throws(() => judgeAudit(REAL, [{ ...PICOMATCH, date }], TODAY), /not a day|no ignoreUntil/, String(date));
  }
  assert.throws(() => judgeAudit(REAL, [PICOMATCH], '2026-11-31'), /not a day/);
  assert.deepEqual(judgeAudit(REAL, [{ ...PICOMATCH, date: '2028-02-29' }], TODAY).problems, []);
});

// The command itself, with an `npm` of the test's own first on the PATH: it writes down what it was asked and prints
// the report it was given. Not judged here: what the real npm answers (the supply-chain job runs it).
function runWithFakeNpm({ printed, toml, env = {} }) {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'npm-audit-test-'));
  try {
    const bin = path.join(dir, 'bin');
    const project = path.join(dir, 'project');
    spawnSync('mkdir', [bin, project]);
    writeFileSync(path.join(dir, 'printed'), printed);
    writeFileSync(
      path.join(bin, 'npm'),
      `#!/bin/sh\nprintf '%s\\n' "$@" > "${dir}/asked"\npwd > "${dir}/where"\ncat "${dir}/printed"\nexit 1\n`,
    );
    chmodSync(path.join(bin, 'npm'), 0o755);
    if (toml !== undefined) writeFileSync(path.join(project, 'osv-scanner.toml'), toml);
    const script = path.join(path.dirname(fileURLToPath(import.meta.url)), 'npm-audit.mjs');
    const run = spawnSync(process.execPath, [script, project], {
      encoding: 'utf8',
      env: { ...process.env, ...env, PATH: `${bin}${path.delimiter}${process.env.PATH}` },
    });
    // Empty when npm was never called: a list refused before the audit.
    const read = (name) => (existsSync(path.join(dir, name)) ? readFileSync(path.join(dir, name), 'utf8') : '');
    return {
      status: run.status,
      out: run.stdout,
      err: run.stderr,
      asked: read('asked').trim().split('\n'),
      where: read('where').trim(),
      project,
    };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
const entry = (date, id = PICOMATCH.id) => `[[IgnoredVulns]]\nid = "${id}"\nignoreUntil = ${date}\nreason = "why"\n`;

test('the command asks npm for every kind of package and for the registry, whatever the environment says', () => {
  const run = runWithFakeNpm({
    printed: JSON.stringify(REAL),
    env: { NODE_ENV: 'production', npm_config_omit: 'dev', npm_config_offline: 'true' },
  });
  assert.deepEqual(run.asked, AUDIT);
  for (const option of [
    'audit',
    '--json',
    '--include=dev',
    '--include=optional',
    '--include=peer',
    '--offline=false',
  ]) {
    assert.ok(run.asked.includes(option), option);
  }
  assert.ok(!run.asked.some((option) => /^--(omit|only|production|audit-level)/.test(option)));
  assert.equal(path.basename(run.where), 'project');
});

test('the command fails on an advisory, passes with its exception of the directory, and fails on what it cannot read', () => {
  const printed = JSON.stringify(REAL);
  const bare = runWithFakeNpm({ printed });
  assert.equal(bare.status, 1);
  assert.match(bare.err, /^GHSA-fc5q-39xc-vjpf \(high, picomatch: /);

  const excepted = runWithFakeNpm({ printed, toml: entry('2999-01-01') });
  assert.equal(excepted.status, 0, excepted.err);
  assert.match(excepted.out, /ignored: GHSA-fc5q-39xc-vjpf .*ignored until 2999-01-01/);

  assert.equal(runWithFakeNpm({ printed, toml: entry('2000-01-01') }).status, 1);
  assert.equal(runWithFakeNpm({ printed, toml: entry('2999-01-01', ghsa(9)) }).status, 1);
  assert.equal(runWithFakeNpm({ printed: JSON.stringify(report([])) }).status, 0);
  // Not a report, or not a list: an error, never a pass.
  for (const broken of ['', 'npm error', '{"error":{"code":"ENOLOCK"}}', printed.slice(0, -20)]) {
    assert.notEqual(runWithFakeNpm({ printed: broken, toml: entry('2999-01-01') }).status, 0, broken.slice(0, 20));
  }
  for (const toml of [
    entry('2999-01-01') + entry('2999-01-01'),
    entry('2999-13-01'),
    '[[IgnoredVulns]]\nreason = "x"\n',
  ]) {
    assert.notEqual(runWithFakeNpm({ printed, toml }).status, 0, toml);
    // Even with nothing to ignore: a list that cannot be read is never taken for an empty one.
    assert.notEqual(runWithFakeNpm({ printed: JSON.stringify(report([])), toml }).status, 0, toml);
  }
});
