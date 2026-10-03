// Tests of ci-plan.mjs: a change of code runs the jobs that check it, a change of documentation runs the always-on
// jobs only, an unknown kind of file runs everything, ci.yml waits for and conditions every job the plan knows, and
// the verdict fails on a planned job that did not pass. Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { JOBS, READ_BY_UI_TESTS, planFor, verdict } from './ci-plan.mjs';

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const jobsOf = (event, files) => planFor(event, files).jobs;
// Every job a change of files can schedule: the weekly campaign aside, and the fuzzing of pull requests on a push.
const BY_FILES = JOBS.filter((job) => !['fuzz', 'fuzz-batch'].includes(job));

test('a change of server code runs the Rust, Kafka, end-to-end and image jobs, and CodeQL for Rust', () => {
  const plan = planFor('push', ['src/engine/matcher.rs']);
  for (const job of ['rust', 'kafka', 'e2e', 'docker', 'codeql']) assert.ok(plan.jobs.includes(job), job);
  assert.ok(!plan.jobs.includes('ui'));
  assert.deepEqual(plan.codeql, ['rust']);
});

test('a change of the interface runs the UI, end-to-end, Kafka and image jobs, and CodeQL for JavaScript', () => {
  const plan = planFor('push', ['frontend/src/lib/components/RuleForm.svelte']);
  for (const job of ['ui', 'e2e', 'kafka', 'docker', 'codeql']) assert.ok(plan.jobs.includes(job), job);
  assert.ok(!plan.jobs.includes('rust'));
  assert.deepEqual(plan.codeql, ['javascript-typescript']);
});

test('a change of an end-to-end spec runs the end-to-end jobs, not the unit tests nor the image', () => {
  const jobs = jobsOf('push', ['frontend/e2e/rule-tester.spec.js']);
  assert.ok(jobs.includes('e2e') && jobs.includes('kafka'));
  assert.ok(!jobs.includes('ui') && !jobs.includes('docker') && !jobs.includes('rust'));
});

test('a change of documentation runs the always-on jobs only', () => {
  const docs = ['README.md', 'docs/en/matching-rules.md', 'docs/fr/screenshots/rule-form.png', 'frontend/README.md'];
  assert.deepEqual(planFor('push', docs), { jobs: ['checks', 'secrets'], codeql: [] });
});

test('a pull request of documentation also has its workflows analysed by CodeQL, for the SAST check', () => {
  assert.deepEqual(planFor('pull_request', ['docs/en/security.md']), {
    jobs: ['checks', 'secrets', 'codeql'],
    codeql: ['actions'],
  });
});

test('a pull request that changes Rust or the fuzz targets is fuzzed; a push is not', () => {
  assert.ok(jobsOf('pull_request', ['src/tcp/hex.rs']).includes('fuzz'));
  assert.ok(jobsOf('pull_request', ['fuzz/fuzz_targets/tcp_hex.rs']).includes('fuzz'));
  assert.ok(jobsOf('pull_request', ['.clusterfuzzlite/build.sh']).includes('fuzz'));
  assert.ok(!jobsOf('push', ['src/tcp/hex.rs']).includes('fuzz'));
  assert.ok(!jobsOf('pull_request', ['frontend/src/App.svelte']).includes('fuzz'));
});

test('a lock file or a policy runs the supply chain job', () => {
  for (const file of ['Cargo.lock', 'frontend/package-lock.json', 'deny.toml', 'osv-scanner.toml']) {
    assert.ok(jobsOf('push', [file]).includes('supply-chain'), file);
  }
  assert.ok(!jobsOf('push', ['src/main.rs']).includes('supply-chain'));
});

test('the Kubernetes manifests and the workflows run their own checks', () => {
  assert.deepEqual(jobsOf('push', ['k8s/base/deployment.yaml']), ['checks', 'secrets', 'kubernetes']);
  assert.deepEqual(planFor('push', ['.github/workflows/release.yml']), {
    jobs: ['checks', 'secrets', 'workflows', 'codeql'],
    codeql: ['actions'],
  });
});

test('an unknown kind of file, the CI workflow or the plan itself runs every job', () => {
  for (const file of ['tool-of-tomorrow.toml', '.github/workflows/ci.yml', 'scripts/ci-plan.mjs']) {
    assert.deepEqual(jobsOf('push', ['README.md', file]), BY_FILES, file);
  }
  assert.deepEqual(jobsOf('push', null), BY_FILES);
  assert.deepEqual(jobsOf('workflow_dispatch', []), BY_FILES);
  assert.deepEqual(jobsOf('pull_request', null), JOBS.filter((job) => job !== 'fuzz-batch'));
  assert.deepEqual(planFor('workflow_dispatch', []).codeql, ['actions', 'javascript-typescript', 'rust']);
});

test('the weekly run checks the advisories published since, runs every CodeQL analysis and the long fuzzing', () => {
  assert.deepEqual(planFor('schedule', []), {
    jobs: ['supply-chain', 'codeql', 'fuzz-batch'],
    codeql: ['actions', 'javascript-typescript', 'rust'],
  });
});

test('every file outside frontend/ that a UI test reads runs the UI job', () => {
  // A test reads a file of the repository through a relative path that leaves frontend/ ("../../../src/...").
  const dir = path.join(ROOT, 'frontend/src/tests');
  const read = new Set();
  for (const name of readdirSync(dir, { recursive: true })) {
    if (!/\.(js|mjs)$/.test(name)) continue;
    const text = readFileSync(path.join(dir, name), 'utf8');
    for (const [, relative] of text.matchAll(/['"`]((?:\.\.\/){3,}[^'"`]+)['"`]/g)) {
      const file = path.relative(ROOT, path.resolve(dir, path.dirname(name), relative)).split(path.sep).join('/');
      if (!file.startsWith('frontend/')) read.add(file);
    }
  }
  assert.ok(read.size > 0, 'the scan found none: it no longer sees how the tests read files');
  for (const file of read) {
    assert.ok(READ_BY_UI_TESTS.includes(file), `${file} is read by a UI test: list it in READ_BY_UI_TESTS`);
    assert.ok(jobsOf('push', [file]).includes('ui'), file);
  }
});

test('every directory a Rust test reads through CARGO_MANIFEST_DIR runs the Rust job', () => {
  const dirs = new Set();
  const visit = (dir) => {
    for (const entry of readdirSync(path.join(ROOT, dir), { withFileTypes: true })) {
      const relative = `${dir}/${entry.name}`;
      if (entry.isDirectory()) visit(relative);
      else if (entry.name.endsWith('.rs')) {
        const text = readFileSync(path.join(ROOT, relative), 'utf8');
        for (const [, joined] of text.matchAll(/CARGO_MANIFEST_DIR"\)\)\s*\.join\("([^"]+)"\)/g)) dirs.add(joined);
      }
    }
  };
  visit('src');
  assert.ok(dirs.size > 0, 'the scan found none: it no longer sees how the tests read files');
  for (const dir of dirs) assert.ok(jobsOf('push', [`${dir}/any-file`]).includes('rust'), dir);
});

// The jobs of ci.yml, with the text of each, read line by line: the workflow keeps one job per two-space key.
function workflowJobs() {
  const text = readFileSync(path.join(ROOT, '.github/workflows/ci.yml'), 'utf8').replace(/\r\n/g, '\n');
  const body = text.slice(text.indexOf('\njobs:\n'));
  const jobs = new Map();
  let current = null;
  for (const line of body.split('\n').slice(2)) {
    const key = line.match(/^ {2}([a-z0-9-]+):\s*$/);
    if (key) jobs.set((current = key[1]), '');
    else if (current) jobs.set(current, `${jobs.get(current)}${line}\n`);
  }
  return jobs;
}

function needsOf(jobText) {
  const inline = jobText.match(/^ {4}needs: \[([^\]]*)\]/m);
  if (inline) return inline[1].split(',').map((name) => name.trim());
  const single = jobText.match(/^ {4}needs: ([a-z0-9-]+)\s*$/m);
  if (single) return [single[1]];
  const block = jobText.match(/^ {4}needs:\n((?: {6}- [a-z0-9-]+\n)+)/m);
  return block ? block[1].split('\n').filter(Boolean).map((line) => line.replace(/^ {6}- /, '')) : [];
}

test('ci.yml runs every job of the plan only when planned, and "CI passed" waits for all of them', () => {
  const jobs = workflowJobs();
  assert.deepEqual([...jobs.keys()].filter((job) => !['changes', 'ci-passed'].includes(job)).sort(), [...JOBS].sort());
  for (const job of JOBS) {
    const text = jobs.get(job);
    assert.ok(needsOf(text).includes('changes'), `${job} needs the plan`);
    const condition = `contains\\(fromJSON\\(needs\\.changes\\.outputs\\.jobs\\), '${job}'\\)`;
    assert.match(text, new RegExp(`^ {4}if: .*${condition}`, 'm'), `${job} runs only when planned`);
  }
  const verdictJob = jobs.get('ci-passed');
  assert.deepEqual(needsOf(verdictJob).sort(), ['changes', ...JOBS].sort());
  assert.match(verdictJob, /^ {4}if: always\(\)\s*$/m);
});

test('the verdict fails on a planned job that failed, was cancelled or was skipped, and on a failed plan', () => {
  const needs = (results) => Object.fromEntries(Object.entries(results).map(([job, result]) => [job, { result }]));
  const all = { changes: 'success', checks: 'success', rust: 'skipped', docker: 'skipped' };
  assert.deepEqual(verdict(needs(all), ['checks']), []);
  assert.deepEqual(verdict(needs(all), ['checks', 'rust']), ['rust: planned, but skipped']);
  assert.deepEqual(verdict(needs({ ...all, rust: 'failure' }), ['checks', 'rust']), ['rust: failure']);
  assert.deepEqual(verdict(needs({ ...all, docker: 'cancelled' }), ['checks']), ['docker: cancelled']);
  assert.deepEqual(verdict(needs({ ...all, changes: 'failure' }), []), ['changes: failure']);
  assert.deepEqual(verdict(needs(all), ['checks', 'e2e']), ['e2e: planned, but "CI passed" does not wait for it']);
});
