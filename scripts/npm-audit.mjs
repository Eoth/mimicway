// Audits every package of a directory's package-lock.json, development tools included, and fails from a high
// advisory on, as `npm audit --audit-level=high` does, but for the advisories the directory's osv-scanner.toml
// ignores: `npm audit` has no list of exceptions, so an advisory without a fixed release kept the job red, and with
// it any other advisory that came. One list serves both tools (osv-scanner reads it for the same lock file), so an
// exception can never hold for one and not the other; each entry carries its reason and its `ignoreUntil`, at most
// six months ahead (scripts/check-exception-dates.mjs), and stops counting on that day.
//
// What is refused rather than passed: a report npm did not give or that cannot be read whole, a high or critical
// advisory without an identifier an exception could name, a package said vulnerable that no advisory of the report
// explains, and an exception without a real date or listed twice. Lower severities do not fail, as before.
//
// It takes the directory and nothing else: no option weakens it, and scripts/check-workflows.mjs wants it alone in
// its step. What it does not see: a registry that answers with fewer advisories than it holds, and a workflow whose
// job would not run at all (an `if:` of the job).
// Run with: node scripts/npm-audit.mjs frontend; its tests: node --test scripts/*.test.mjs
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { osvExceptions } from './check-exception-dates.mjs';

// Every kind of package, and the registry really asked: said on the command line, which wins over the environment
// and over an .npmrc (`omit=dev`, which NODE_ENV=production also sets, would leave the build tools out, and
// `offline=true` gives an empty report without asking anything).
export const AUDIT = [
  'audit',
  '--json',
  '--include=prod',
  '--include=dev',
  '--include=optional',
  '--include=peer',
  '--offline=false',
];

const FAILING = ['high', 'critical'];
const SEVERITIES = ['info', 'low', 'moderate', ...FAILING];

/** Whether `text` is a day of the calendar written YYYY-MM-DD: two such texts then compare as their days do. */
const isDay = (text) =>
  typeof text === 'string' &&
  /^\d{4}-\d{2}-\d{2}$/.test(text) &&
  !Number.isNaN(Date.parse(`${text}T00:00:00Z`)) &&
  new Date(`${text}T00:00:00Z`).toISOString().slice(0, 10) === text;

/** The identifier of the advisory an entry of `via` describes (`GHSA-…`, as osv-scanner names it), or null. */
const advisoryId = (via) =>
  String(via.url ?? '').match(/^https:\/\/github\.com\/advisories\/(GHSA(?:-[0-9a-z]{4}){3})$/)?.[1] ?? null;

/**
 * The verdict on a report of `npm audit --json`, with the exceptions of osv-scanner.toml (`{ id, date }`) on
 * `today` (an ISO date): the problems, the advisories that fail the audit, and the ones an exception covers.
 * Throws on a report that is not one, and on a list of exceptions that cannot be trusted.
 */
export function judgeAudit(report, exceptions, today) {
  const packages = report?.vulnerabilities;
  const counts = report?.metadata?.vulnerabilities;
  if (report === null || typeof report !== 'object' || report.error !== undefined) {
    throw new Error('npm audit gave no report');
  }
  if (packages === null || typeof packages !== 'object' || Array.isArray(packages) || !counts) {
    throw new Error('the report of npm audit has no list of vulnerable packages or no counts');
  }
  if (!isDay(today)) throw new Error(`not a day: ${today}`);
  const entries = Object.values(packages);
  for (const entry of entries) {
    if (!SEVERITIES.includes(entry?.severity) || !Array.isArray(entry.via) || typeof entry.name !== 'string') {
      throw new Error('the report of npm audit holds a package entry that cannot be read');
    }
  }
  // The counts npm gives are of packages: the list read must be the whole of it.
  for (const severity of SEVERITIES) {
    if (entries.filter((entry) => entry.severity === severity).length !== counts[severity]) {
      throw new Error(
        `the report of npm audit counts ${counts[severity]} ${severity} packages, the list holds another number`,
      );
    }
  }

  // An identifier listed twice could be read one way here and another by osv-scanner: it is listed once.
  const covering = new Map();
  const listed = new Set();
  for (const { id, date } of exceptions) {
    if (listed.has(id)) throw new Error(`the exception ${id} is listed twice`);
    listed.add(id);
    if (!date) throw new Error(`the exception ${id} has no ignoreUntil`);
    if (!isDay(date)) throw new Error(`the ignoreUntil of the exception ${id} is not a day: ${date}`);
    if (today < date) covering.set(id, date);
  }

  const problems = [];
  const failing = new Map();
  const excepted = new Map();
  for (const entry of entries) {
    for (const via of entry.via) {
      if (typeof via === 'string') continue;
      if (via === null || typeof via !== 'object' || !SEVERITIES.includes(via.severity)) {
        throw new Error(`the report of npm audit holds an advisory of ${entry.name} that cannot be read`);
      }
      if (!FAILING.includes(via.severity)) continue;
      const id = advisoryId(via);
      const label = `${id ?? via.url ?? 'an advisory without address'} (${via.severity}, ${entry.name}: ${via.title ?? 'no title'})`;
      if (id && covering.has(id)) {
        excepted.set(id, `${label}, ignored until ${covering.get(id)}`);
      } else {
        failing.set(id ?? label, label);
      }
    }
  }
  // The severity of a package is the worst of the advisories it leads to: its own, or those of the packages its
  // `via` names. A package said high or critical that leads to nothing as severe has an advisory the report does not
  // show, which no exception was judged for.
  const worstReached = (name, seen = new Set()) => {
    if (seen.has(name)) return -1;
    seen.add(name);
    const reached = (packages[name]?.via ?? []).map((via) =>
      typeof via === 'string' ? worstReached(via, seen) : SEVERITIES.indexOf(via.severity),
    );
    return Math.max(-1, ...reached);
  };
  for (const entry of entries) {
    if (FAILING.includes(entry.severity) && worstReached(entry.name) < SEVERITIES.indexOf(entry.severity)) {
      problems.push(`${entry.name} is said ${entry.severity} and no advisory of the report explains it`);
    }
  }
  for (const label of failing.values()) problems.push(label);
  return {
    problems,
    failing: [...failing.values()],
    excepted: [...excepted.values()],
  };
}

/** What `npm audit --json` prints for the lock file of `directory`; it exits 1 as soon as it finds an advisory. */
function auditReport(directory) {
  const run = spawnSync('npm', AUDIT, {
    cwd: directory,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  if (run.error) throw run.error;
  try {
    return JSON.parse(run.stdout);
  } catch {
    throw new Error(`npm audit printed no JSON (exit ${run.status}): ${run.stderr.trim().split('\n').pop()}`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.length !== 1 || args[0].startsWith('-')) {
    console.error('usage: node scripts/npm-audit.mjs <directory of a package-lock.json>');
    process.exit(2);
  }
  const directory = path.resolve(args[0]);
  let exceptions = [];
  try {
    exceptions = osvExceptions(readFileSync(path.join(directory, 'osv-scanner.toml'), 'utf8'));
  } catch (error) {
    // No list is no exception; a list that cannot be read is an error.
    if (error.code !== 'ENOENT') throw error;
  }
  const today = new Date().toISOString().slice(0, 10);
  const verdict = judgeAudit(auditReport(directory), exceptions, today);
  for (const line of verdict.excepted) console.log(`ignored: ${line}`);
  for (const problem of verdict.problems) console.error(problem);
  if (verdict.problems.length > 0) process.exit(1);
  console.log(
    `npm audit of ${args[0]}: no high or critical advisory` +
      (verdict.excepted.length > 0 ? ` but the ${verdict.excepted.length} ignored above (osv-scanner.toml).` : '.'),
  );
}
