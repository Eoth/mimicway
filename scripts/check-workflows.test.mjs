// Tests of check-workflows.mjs: a closed workflow that passes, each way of leaving a credential within reach that
// fails, and the files of the repository. Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkWorkflow, jobsOf } from './check-workflows.mjs';

const workflow = ({
  checkoutWith = '        with:\n          persist-credentials: false\n',
  publish = '',
} = {}) => `name: T
on: push
jobs:
  build:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@0123456789abcdef # v7
${checkoutWith}      - uses: actions/setup-node@0123456789abcdef
        with:
          persist-credentials: false
          node-version: 24
      - run: echo ok
${publish}`;

const PUBLISH = `  release:
    runs-on: ubuntu-24.04
    environment: release
    permissions:
      contents: write
    steps:
      - run: gh release create
`;

test('a workflow whose checkouts keep no token and whose publishing job has an environment passes', () => {
  const result = checkWorkflow('t.yml', workflow({ publish: PUBLISH }));
  assert.deepEqual(result.problems, []);
  assert.deepEqual([result.jobs, result.checkouts, result.publishers], [2, 1, 1]);
});

test('a checkout without persist-credentials fails, even when the next step sets it', () => {
  const { problems } = checkWorkflow('t.yml', workflow({ checkoutWith: '' }));
  assert.deepEqual(problems, ["t.yml, job build: a checkout keeps the job's token (set persist-credentials: false)"]);
});

test('a checkout with other settings but not persist-credentials fails', () => {
  const { problems } = checkWorkflow('t.yml', workflow({ checkoutWith: '        with:\n          fetch-depth: 0\n' }));
  assert.equal(problems.length, 1);
});

test('a job that writes contents or packages outside an environment fails', () => {
  for (const scope of ['contents', 'packages']) {
    const open = PUBLISH.replace('    environment: release\n', '').replace('contents: write', `${scope}: write`);
    assert.deepEqual(checkWorkflow('t.yml', workflow({ publish: open })).problems, [
      't.yml, job release: it may write contents or packages outside a protected environment',
    ]);
  }
});

test('an environment named in a step does not count for the job', () => {
  const open = PUBLISH.replace('    environment: release\n', '').replace(
    '      - run: gh release create\n',
    '      - run: gh release create\n        environment: release\n',
  );
  assert.equal(checkWorkflow('t.yml', workflow({ publish: open })).problems.length, 1);
});

test('a file without jobs is refused rather than passed', () => {
  assert.throws(() => jobsOf('name: T\non: push\n'), /no "jobs:"/);
});

test('every workflow of the repository is closed', () => {
  const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '.github', 'workflows');
  const files = readdirSync(dir).filter((name) => /\.ya?ml$/.test(name));
  assert.ok(files.includes('release.yml'));
  for (const file of files) {
    const result = checkWorkflow(file, readFileSync(path.join(dir, file), 'utf8'));
    assert.deepEqual(result.problems, [], file);
    assert.ok(result.checkouts > 0, `${file}: no checkout read`);
  }
});

const audit = (command) => workflow({ publish: '' }) + `      - run: ${command}\n`;

test('an npm audit passes only when it fails from high on and audits every package', () => {
  const verdicts = [
    ['npm audit --audit-level=high', 0],
    ['npm audit --audit-level=moderate', 0],
    ['npm audit --audit-level low', 0],
    ['npm audit --audit-level=critical', 1],
    ['npm audit', 1],
    ['npm audit --omit=dev --audit-level=high', 1],
    ['npm audit --production --audit-level=critical', 2],
  ];
  for (const [command, problems] of verdicts) {
    const result = checkWorkflow('t.yml', audit(command));
    assert.equal(result.problems.length, problems, command);
    assert.equal(result.audits, 1, command);
  }
});

const release = (uploadWith, buildEnv) => `name: Release
on: push
jobs:
  binary:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/download-artifact@0123456789abcdef
        with:
          name: ui
      - run: cargo build
      # The binary, for the jobs after this one.
      - uses: actions/upload-artifact@0123456789abcdef # v7
        with:
          name: binary
${uploadWith}  image:
    runs-on: ubuntu-24.04
    steps:
      - id: build
        uses: docker/build-push-action@0123456789abcdef # v7
${buildEnv}        with:
          push: true
`;
const KEPT_A_WEEK = '          path: out/mimicway\n          retention-days: 7\n';
const RECORD_A_WEEK = '        env:\n          DOCKER_BUILD_RECORD_RETENTION_DAYS: 7\n';

test('the release workflow passes when each upload and build record expires within a week', () => {
  const result = checkWorkflow('release.yml', release(KEPT_A_WEEK, RECORD_A_WEEK));
  assert.deepEqual(result.problems, []);
  assert.equal(result.shortLived, 2);
});

test('an upload of the release workflow without retention-days, or kept longer, fails', () => {
  for (const uploadWith of ['          path: out/mimicway\n', '          path: out\n          retention-days: 30\n']) {
    const result = checkWorkflow('release.yml', release(uploadWith, RECORD_A_WEEK));
    assert.equal(result.problems.length, 1, uploadWith);
    assert.match(result.problems[0], /job binary: an artifact kept longer than a week/);
  }
});

test('a Docker build of the release workflow whose record keeps the default fails', () => {
  const result = checkWorkflow('release.yml', release(KEPT_A_WEEK, ''));
  assert.equal(result.problems.length, 1);
  assert.match(result.problems[0], /job image: a Docker build record kept longer than a week/);
});

test('the CI keeps the default retention of its failure reports', () => {
  const result = checkWorkflow('ci.yml', release('          path: out/mimicway\n', ''));
  assert.deepEqual(result.problems, []);
  assert.equal(result.shortLived, 0);
});

test('a release workflow where no upload is found is refused rather than passed', () => {
  const result = checkWorkflow('release.yml', workflow());
  assert.match(result.problems.join('\n'), /no artifact upload nor Docker build found/);
});
