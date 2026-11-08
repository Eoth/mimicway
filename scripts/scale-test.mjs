#!/usr/bin/env node
// The scale test: runs every family of src/scale_probe.rs at n, 2n and 4n under Callgrind, which counts the same
// instructions on every run whatever the load of the machine, and fails when a family's count grows from n to 4n faster
// than the growth written next to it allows. A step that turns quadratic (a list scanned once per element, a log copied
// on each write) shows here long before anyone notices it on a large configuration.
//
// What it does not see: a cost that grows only beyond 4n, or only with an input shape no family builds; time spent
// waiting (disk, network, locks), which Callgrind does not count; and a fixed cost that grew, since only the ratio is
// judged (absolute counts change with the compiler).
//
// Usage: node scripts/scale-test.mjs [family...]     needs valgrind; builds the probe with cargo (profile "scale")
import { execFileSync, spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

// The one tolerance: a family may grow up to this factor more than its class allows from n to 4n (heap layout and
// fixed costs move the count a little), and must grow at least 4 / TOLERANCE, or its fixed cost hides the growth it is
// meant to show. A share of quadratic work above about 8 % of the count at n is then enough to turn a linear family red.
export const TOLERANCE = 1.25;

// The C library's allocator without its per-thread cache and fast bins: with them, how much work a free or a malloc
// does jumps with the size of the heap (merging free blocks, at 2n and not 4n for instance), by more than the
// tolerance. Its instructions stay counted: a step that copies or allocates more is still seen.
export const GLIBC_TUNABLES = 'glibc.malloc.tcache_count=0:glibc.malloc.mxfast=0';

// The growth from n to 4n that each class of src/scale_probe.rs allows, before the tolerance.
export function classRatio(growth, n) {
  if (growth === 'linear') return 4;
  if (growth === 'n log n') return (4 * Math.log2(4 * n)) / Math.log2(n);
  throw new Error(`unknown growth class: ${growth}`);
}

// The instructions Callgrind collected, from its output file.
export function parseTotals(text) {
  const match = /^totals:\s+(\d+)/m.exec(text) ?? /^summary:\s+(\d+)/m.exec(text);
  if (!match) throw new Error('no totals line in the Callgrind output');
  return Number(match[1]);
}

// The verdict of one family from its three counts (n, 2n, 4n).
export function judge({ growth, n, counts }) {
  const [c1, , c4] = counts;
  if (!(c1 > 0) || !(c4 > 0))
    return {
      ok: false,
      ratio: NaN,
      reason: 'nothing counted: `measured` was not reached',
    };
  const ratio = c4 / c1;
  const allowed = classRatio(growth, n) * TOLERANCE;
  const floor = 4 / TOLERANCE;
  if (ratio > allowed) {
    return {
      ok: false,
      ratio,
      allowed,
      reason: `grows ${ratio.toFixed(2)}x from n to 4n, more than ${growth} allows`,
    };
  }
  if (ratio < floor) {
    return {
      ok: false,
      ratio,
      allowed,
      reason: `grows ${ratio.toFixed(2)}x only: its fixed cost hides the growth, raise n`,
    };
  }
  return { ok: true, ratio, allowed };
}

// Families as the probe lists them: name, n, growth class, what it measures.
export function parseFamilies(text) {
  return text
    .trim()
    .split('\n')
    .map((line) => {
      const [name, n, growth, about] = line.split('\t');
      return { name, n: Number(n), growth, about };
    });
}

function buildProbe() {
  const out = execFileSync(
    'cargo',
    [
      'build',
      '--locked',
      '--profile',
      'scale',
      '--features',
      'scale-probe',
      '--test',
      'scale',
      '--message-format=json',
    ],
    {
      cwd: ROOT,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
      maxBuffer: 64 * 1024 * 1024,
    },
  );
  for (const line of out.split('\n')) {
    if (!line.startsWith('{')) continue;
    const message = JSON.parse(line);
    if (message.reason === 'compiler-artifact' && message.target?.name === 'scale' && message.executable) {
      return message.executable;
    }
  }
  throw new Error('cargo built no scale probe');
}

function callgrind(probe, family, n, dir) {
  const out = path.join(dir, `${family}-${n}.out`);
  const args = [
    '--tool=callgrind',
    '--collect-atstart=no',
    '--toggle-collect=*scale_probe::measured*',
    `--callgrind-out-file=${out}`,
    probe,
    'probe',
    family,
    String(n),
  ];
  return new Promise((resolve, reject) => {
    const child = spawn('valgrind', args, {
      stdio: ['ignore', 'ignore', 'pipe'],
      env: { ...process.env, GLIBC_TUNABLES },
    });
    let stderr = '';
    child.stderr.on('data', (chunk) => (stderr += chunk));
    child.on('error', reject);
    child.on('close', (code) => {
      if (code !== 0) return reject(new Error(`${family} at ${n}: valgrind exited with ${code}\n${stderr}`));
      try {
        resolve(parseTotals(readFileSync(out, 'utf8')));
      } catch (e) {
        reject(new Error(`${family} at ${n}: ${e.message}`));
      }
    });
  });
}

// Runs `tasks` (functions returning promises) with at most `limit` at a time, results in order.
async function pool(tasks, limit) {
  const results = new Array(tasks.length);
  let next = 0;
  const worker = async () => {
    while (next < tasks.length) {
      const i = next++;
      results[i] = await tasks[i]();
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, tasks.length) }, worker));
  return results;
}

async function main(selected) {
  try {
    execFileSync('valgrind', ['--version'], { stdio: 'ignore' });
  } catch {
    console.error('valgrind is needed (apt-get install valgrind)');
    return 2;
  }
  const probe = buildProbe();
  let families = parseFamilies(execFileSync(probe, ['families'], { encoding: 'utf8' }));
  if (selected.length > 0) {
    const unknown = selected.filter((name) => !families.some((f) => f.name === name));
    if (unknown.length > 0) {
      console.error(`unknown families: ${unknown.join(', ')}`);
      return 2;
    }
    families = families.filter((f) => selected.includes(f.name));
  }
  const dir = mkdtempSync(path.join(os.tmpdir(), 'mimicway-scale-'));
  try {
    const tasks = families.flatMap((f) => [1, 2, 4].map((k) => () => callgrind(probe, f.name, f.n * k, dir)));
    const counts = await pool(tasks, os.availableParallelism());
    let failed = 0;
    families.forEach((f, i) => {
      const c = counts.slice(3 * i, 3 * i + 3);
      const verdict = judge({ growth: f.growth, n: f.n, counts: c });
      const sizes = c.map((count, k) => `${f.n * 2 ** k}: ${count.toLocaleString('en-US')}`).join(', ');
      const status = verdict.ok ? 'ok ' : 'RED';
      console.log(
        `${status} ${f.name} (${f.growth}, allowed ${verdict.allowed?.toFixed(2)}x): ${verdict.ratio.toFixed(2)}x`,
      );
      console.log(`    ${f.about}`);
      console.log(`    ${sizes}`);
      if (!verdict.ok) {
        failed++;
        console.error(`::error::scale test, ${f.name}: ${verdict.reason}`);
      }
    });
    return failed === 0 ? 0 : 1;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await main(process.argv.slice(2));
}
