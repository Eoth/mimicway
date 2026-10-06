#!/usr/bin/env node
// Decides which jobs of the CI workflow a change needs, and gives the verdict of the "CI passed" job, the one check
// branch protection requires.
//
// Each job runs only when a file it checks changes: a change to the guide does not rebuild the container image. A path
// that no rule below claims, and that is not documentation, makes every job run: a new kind of file is checked by
// everything until a rule says otherwise. The verdict fails when a planned job did not pass, skipped included, so a
// job that a wrong condition skips cannot pass for a green run.
//
// Usage (in .github/workflows/ci.yml):
//   node scripts/ci-plan.mjs plan      writes `jobs` and `codeql` (JSON arrays: jobs, CodeQL languages) to
//                                      $GITHUB_OUTPUT. Reads GITHUB_EVENT_NAME and BEFORE (the commit a push starts
//                                      from).
//   node scripts/ci-plan.mjs verdict   reads NEEDS (toJSON(needs)) and PLANNED (the `jobs` output), exits 1 on a
//                                      planned job that did not pass, or on any failed or cancelled job.
import { execFileSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Jobs of ci.yml that the plan schedules, besides `changes` (which computes the plan) and `ci-passed` (the verdict).
export const JOBS = [
  'checks',
  'secrets',
  'rust',
  'kafka',
  'supply-chain',
  'ui',
  'format',
  'e2e',
  'docker',
  'kubernetes',
  'workflows',
  'codeql',
  'fuzz',
  'fuzz-batch',
];

// Always run on a push or a pull request: any file can hold a secret, a broken link or the former design system's
// name, and both jobs take seconds.
const ALWAYS = ['checks', 'secrets'];

const CODEQL_LANGUAGES = ['actions', 'javascript-typescript', 'rust'];

// Read and checked by people only: a change to these runs the always-on jobs alone.
const DOCUMENTATION = [/\.md$/, /^docs\//, /^LICENSE$/, /^\.github\/ISSUE_TEMPLATE\//];

const RUST = [
  /^src\//,
  /^tests\//,
  /^examples\//,
  /^build\.rs$/,
  /^Cargo\.(toml|lock)$/,
  /^\.cargo\//,
  /^rust-toolchain(\.toml)?$/,
  /^\.?(rustfmt|clippy)\.toml$/,
];
const MANIFESTS = [/^Cargo\.(toml|lock)$/, /^frontend\/package(-lock)?\.json$/];
const FUZZING = [/^fuzz\//, /^\.clusterfuzzlite\//];
// Files outside frontend/ that the UI's unit tests read: frontend/src/tests/UrlHealthBadge.test.js compares the
// server's PING_TTL_MS with its own. ci-plan.test.mjs fails when a UI test reads another file that is not listed here.
export const READ_BY_UI_TESTS = ['src/server/ping.rs'];

// For each job, the paths it checks. A path may belong to several jobs.
const RULES = {
  rust: RUST,
  // Builds the UI too: the Kafka specs of the end-to-end suite run against this job's binary.
  kafka: [...RUST, /^frontend\//],
  'supply-chain': [...MANIFESTS, /^deny\.toml$/, /^osv-scanner\.toml$/],
  ui: [/^frontend\/(?!e2e\/)/, ...READ_BY_UI_TESTS.map((file) => new RegExp(`^${escape(file)}$`))],
  // What `npm run format:check` reads: the code of the interface, end-to-end tests included, and of the repository's
  // scripts, and what decides the formatting (its configuration, and its version in the lock file).
  format: [
    /^frontend\/.*\.(js|mjs|cjs|svelte|css|html)$/,
    /^frontend\/(\.prettierignore|package(-lock)?\.json)$/,
    /^scripts\/.*\.(js|mjs)$/,
  ],
  e2e: [...RUST, /^frontend\//],
  // What the Dockerfile copies: the UI (its end-to-end suite aside) and the server.
  docker: [/^Dockerfile$/, /^\.dockerignore$/, /^Cargo\.(toml|lock)$/, /^build\.rs$/, /^src\//, /^frontend\/(?!e2e\/)/],
  kubernetes: [/^k8s\//],
  workflows: [/^\.github\/workflows\//, /^\.github\/actions\//],
  fuzz: [...RUST, ...FUZZING],
};
// For each language CodeQL analyses, its files.
const CODEQL_RULES = {
  actions: [/^\.github\/workflows\//, /^\.github\/actions\//],
  'javascript-typescript': [/^frontend\/.*\.(js|mjs|cjs|ts|svelte|html)$/, /^scripts\/.*\.(js|mjs)$/],
  rust: [/^src\/.*\.rs$/, /^tests\/.*\.rs$/, /^build\.rs$/, /^fuzz\/.*\.rs$/, /^Cargo\.(toml|lock)$/],
};
// Checked by the always-on jobs, or by no job at all: claimed so that changing them does not run everything.
// Dockerfile.release is built by the release workflow only, on a tag; the bootstrap scripts are run by people;
// .git-blame-ignore-revs lists the commits that git blame skips.
const ALWAYS_ONLY = [
  /^scripts\//,
  /^Dockerfile\.release$/,
  /^\.gitignore$/,
  /^\.gitattributes$/,
  /^\.git-blame-ignore-revs$/,
  /^\.editorconfig$/,
  /^\.github\/dependabot\.yml$/,
];
// Define what runs: a change to them runs every job, the proof that the CI still works.
const EVERYTHING = [/^\.github\/workflows\/ci\.yml$/, /^scripts\/ci-plan\.mjs$/];

function escape(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

const matches = (patterns, file) => patterns.some((pattern) => pattern.test(file));

// The jobs and CodeQL languages a set of changed files needs, for a push or a pull request. `files` null means the
// changes are unknown (a new branch, a force push whose start is gone): everything runs.
export function planFor(event, files) {
  if (event === 'schedule') {
    // The weekly run: advisories published against unchanged lock files, the queries CodeQL added since, and the long
    // fuzzing campaign.
    return { jobs: ['supply-chain', 'codeql', 'fuzz-batch'], codeql: [...CODEQL_LANGUAGES] };
  }
  const jobs = new Set(ALWAYS);
  const codeql = new Set();
  const everything = event === 'workflow_dispatch' || files === null || files.some((f) => matches(EVERYTHING, f));
  if (everything) {
    for (const job of Object.keys(RULES)) jobs.add(job);
    for (const language of CODEQL_LANGUAGES) codeql.add(language);
  } else {
    for (const file of files) {
      if (matches(DOCUMENTATION, file)) continue;
      let claimed = matches(ALWAYS_ONLY, file);
      for (const [job, patterns] of Object.entries(RULES)) {
        if (matches(patterns, file)) {
          jobs.add(job);
          claimed = true;
        }
      }
      for (const [language, patterns] of Object.entries(CODEQL_RULES)) {
        if (matches(patterns, file)) codeql.add(language);
      }
      if (!claimed) return planFor(event, null);
    }
  }
  // Scorecard's SAST check counts the merged pull requests that carry a successful CodeQL check: the workflows are
  // analysed on every pull request, documentation included, which takes about a minute.
  if (event === 'pull_request') codeql.add('actions');
  // ClusterFuzzLite fuzzes what a pull request changes; what reaches develop otherwise waits for the weekly campaign.
  if (event !== 'pull_request') jobs.delete('fuzz');
  if (codeql.size > 0) jobs.add('codeql');
  return {
    jobs: JOBS.filter((job) => jobs.has(job)),
    codeql: CODEQL_LANGUAGES.filter((language) => codeql.has(language)),
  };
}

// What is wrong with a finished run: `needs` is the workflow's needs context, `planned` the jobs the plan scheduled.
export function verdict(needs, planned) {
  const problems = [];
  for (const job of planned) {
    if (!(job in needs)) problems.push(`${job}: planned, but "CI passed" does not wait for it`);
  }
  for (const [job, { result }] of Object.entries(needs)) {
    if (result === 'failure' || result === 'cancelled') problems.push(`${job}: ${result}`);
    else if ((planned.includes(job) || job === 'changes') && result !== 'success') {
      problems.push(`${job}: planned, but ${result}`);
    }
  }
  return problems;
}

function changedFiles(event, before) {
  const git = (...args) => execFileSync('git', args, { encoding: 'utf8' });
  try {
    // A pull request is checked out as a merge commit: its first parent is the target branch.
    if (event === 'pull_request') return git('diff', '--name-only', 'HEAD^1', 'HEAD').split('\n').filter(Boolean);
    if (event === 'push' && before && !/^0+$/.test(before)) {
      git('fetch', '--no-tags', '--depth=1', 'origin', before);
      return git('diff', '--name-only', before, 'HEAD').split('\n').filter(Boolean);
    }
  } catch (error) {
    console.log(`Changed files unknown (${error.message.split('\n')[0]}): every job runs.`);
  }
  return null;
}

function main(command) {
  if (command === 'plan') {
    const event = process.env.GITHUB_EVENT_NAME;
    const files = ['push', 'pull_request'].includes(event) ? changedFiles(event, process.env.BEFORE) : [];
    const plan = planFor(event, files);
    console.log(`${event}, ${files === null ? 'unknown changes' : `${files.length} changed files`}`);
    for (const file of files ?? []) console.log(`  ${file}`);
    const lines = Object.entries(plan).map(([name, value]) => `${name}=${JSON.stringify(value)}`);
    console.log(lines.join('\n'));
    if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `${lines.join('\n')}\n`);
    return 0;
  }
  if (command === 'verdict') {
    const needs = JSON.parse(process.env.NEEDS);
    const planned = JSON.parse(process.env.PLANNED || '[]');
    for (const [job, { result }] of Object.entries(needs)) console.log(`${job}: ${result}`);
    const problems = verdict(needs, planned);
    for (const problem of problems) console.error(`::error::${problem}`);
    return problems.length === 0 ? 0 : 1;
  }
  console.error('usage: node scripts/ci-plan.mjs plan|verdict');
  return 2;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv[2]);
}
