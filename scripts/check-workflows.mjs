// Fails when a GitHub Actions workflow of .github/workflows/ leaves a publishing credential within reach:
// - every actions/checkout step sets `persist-credentials: false`: otherwise the job's token stays in the cloned
//   repository's Git configuration, where any later step of the job can read it;
// - every job that may write the repository's contents or publish packages (`contents: write`, `packages: write`)
//   runs in an environment, which the maintainers protect with a required reviewer: otherwise a pushed tag publishes
//   without anyone's approval;
// - every `npm audit` fails from a high advisory on and audits every package, development tools included: a high
//   advisory of the build tooling (vite) once went through a step that only failed on critical ones.
// - every artifact the release workflow uploads, and the record of its Docker build, expires within a week: a
//   rehearsal is read the day it runs and a tag publishes copies of them as release assets, while GitHub keeps an
//   artifact 90 days by default (56 MB a run). The CI's reports of a failed run keep the default: they may be read
//   later.
// Every .yml file of .github/workflows/ is read, so a new workflow is checked without being listed, and the summary
// counts what was checked: a reading that found no checkout or no job would show.
// Run with: node scripts/check-workflows.mjs; its tests: node --test scripts/*.test.mjs
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const indentOf = (line) => line.length - line.trimStart().length;
const meaningful = (line) => line.trim() !== '' && !line.trimStart().startsWith('#');

/** The lines under `lines[at]` that are indented deeper than it (its block), comments and blank lines left out. */
function blockUnder(lines, at) {
  const indent = indentOf(lines[at]);
  const block = [];
  for (const line of lines.slice(at + 1)) {
    if (!meaningful(line)) continue;
    if (indentOf(line) <= indent) break;
    block.push(line);
  }
  return block;
}

/**
 * The jobs of a workflow: `{ name, lines }` for each key directly under `jobs:`. Throws when the file has no `jobs:`,
 * so that a check never passes on a file it could not read.
 */
export function jobsOf(text) {
  const lines = text.split('\n');
  const at = lines.findIndex((line) => /^jobs:\s*$/.test(line));
  if (at < 0) throw new Error('no "jobs:" at the top level');
  const jobs = [];
  const block = blockUnder(lines, at);
  const jobIndent = block.length > 0 ? indentOf(block[0]) : 0;
  block.forEach((line, i) => {
    if (indentOf(line) !== jobIndent) return;
    const name = line.trim().replace(/:\s*$/, '');
    const body = [];
    for (const next of block.slice(i + 1)) {
      if (indentOf(next) <= jobIndent) break;
      body.push(next);
    }
    jobs.push({ name, lines: body });
  });
  return jobs;
}

/** The workflows whose artifacts expire within a week. */
export const SHORT_LIVED = ['release.yml'];
const WEEK = 7;

/** The steps of a job: the trimmed lines of each item of its `steps:` list, comments and blank lines left out. */
function stepsOf(jobLines) {
  const at = jobLines.findIndex((line) => /^steps:\s*$/.test(line.trim()));
  if (at < 0) return [];
  const steps = [];
  const block = blockUnder(jobLines, at);
  const itemIndent = block.length > 0 ? indentOf(block[0]) : 0;
  for (const line of block) {
    if (indentOf(line) === itemIndent && line.trimStart().startsWith('-')) steps.push([]);
    steps.at(-1)?.push(line.trim().replace(/^-\s+/, ''));
  }
  return steps;
}

/** The value of `key:` among the lines of a step, as a number, or NaN. */
const numberOf = (step, key) =>
  Number(
    step
      .find((line) => line.startsWith(`${key}:`))
      ?.slice(key.length + 1)
      .trim(),
  );

/** The problems of one workflow (`file` names it in each message), with what was checked. */
export function checkWorkflow(file, text) {
  const problems = [];
  let checkouts = 0;
  let publishers = 0;
  let audits = 0;
  let shortLived = 0;
  const jobs = jobsOf(text);
  for (const job of jobs) {
    if (SHORT_LIVED.includes(file)) {
      for (const step of stepsOf(job.lines)) {
        const uses = step.find((line) => line.startsWith('uses:')) ?? '';
        if (/^uses:\s*actions\/upload-artifact@/.test(uses)) {
          shortLived++;
          if (!(numberOf(step, 'retention-days') <= WEEK)) {
            problems.push(`${file}, job ${job.name}: an artifact kept longer than a week (retention-days: ${WEEK})`);
          }
        }
        if (/^uses:\s*docker\/build-push-action@/.test(uses)) {
          shortLived++;
          if (!(numberOf(step, 'DOCKER_BUILD_RECORD_RETENTION_DAYS') <= WEEK)) {
            problems.push(
              `${file}, job ${job.name}: a Docker build record kept longer than a week ` +
                `(DOCKER_BUILD_RECORD_RETENTION_DAYS: ${WEEK} in the step's env)`,
            );
          }
        }
      }
    }
    for (const line of job.lines) {
      // A command, in a `run:` line or the lines of its block; never the `name:` of a step.
      if (!/\bnpm audit\b/.test(line) || /^\s*(-\s+)?name:/.test(line)) continue;
      audits++;
      const level = line.match(/--audit-level[= ](\w+)/)?.[1];
      if (!['low', 'moderate', 'high'].includes(level)) {
        problems.push(`${file}, job ${job.name}: an npm audit that lets a high advisory through (--audit-level=high)`);
      }
      if (/--omit\b|--only\b|--production\b/.test(line)) {
        problems.push(`${file}, job ${job.name}: an npm audit that leaves packages out (development tools too)`);
      }
    }
    job.lines.forEach((line, i) => {
      if (!/^\s*(-\s+)?uses:\s*actions\/checkout@/.test(line)) return;
      checkouts++;
      const step = [];
      const stepIndent = indentOf(line) + (line.trimStart().startsWith('-') ? 2 : 0);
      for (const next of job.lines.slice(i + 1)) {
        if (indentOf(next) < stepIndent || (indentOf(next) === stepIndent && next.trimStart().startsWith('-'))) break;
        step.push(next.trim());
      }
      if (!step.includes('persist-credentials: false')) {
        problems.push(`${file}, job ${job.name}: a checkout keeps the job's token (set persist-credentials: false)`);
      }
    });
    const writes = job.lines.some((line) => /^\s*(contents|packages):\s*write\s*$/.test(line));
    if (writes) {
      publishers++;
      const jobIndent = indentOf(job.lines[0] ?? '');
      const environment = job.lines.some(
        (line) => indentOf(line) === jobIndent && /^environment:\s*\S/.test(line.trim()),
      );
      if (!environment) {
        problems.push(`${file}, job ${job.name}: it may write contents or packages outside a protected environment`);
      }
    }
  }
  if (SHORT_LIVED.includes(file) && shortLived === 0) {
    problems.push(`${file}: no artifact upload nor Docker build found, where its artifacts are checked`);
  }
  return {
    problems,
    jobs: jobs.length,
    checkouts,
    publishers,
    audits,
    shortLived,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '.github', 'workflows');
  const files = readdirSync(dir).filter((name) => /\.ya?ml$/.test(name));
  const totals = {
    problems: [],
    jobs: 0,
    checkouts: 0,
    publishers: 0,
    audits: 0,
    shortLived: 0,
  };
  for (const file of files) {
    const result = checkWorkflow(file, readFileSync(path.join(dir, file), 'utf8'));
    totals.problems.push(...result.problems);
    totals.jobs += result.jobs;
    totals.checkouts += result.checkouts;
    totals.publishers += result.publishers;
    totals.audits += result.audits;
    totals.shortLived += result.shortLived;
  }
  for (const problem of totals.problems) console.error(problem);
  if (totals.audits === 0) console.error('no npm audit in any workflow: the UI packages are not audited');
  if (totals.problems.length > 0 || files.length === 0 || totals.checkouts === 0 || totals.audits === 0) {
    process.exit(1);
  }
  console.log(
    `${files.length} workflows, ${totals.jobs} jobs: ${totals.checkouts} checkouts keep no token, ` +
      `${totals.publishers} publishing jobs run in an environment, ${totals.audits} npm audits fail from high on, ` +
      `${totals.shortLived} release artifacts expire within a week.`,
  );
}
