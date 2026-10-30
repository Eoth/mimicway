// Fails when an advisory ignored by the supply-chain gates has no date to be looked at again, a date already
// reached, or one more than six months ahead: an exception written once would otherwise stay for good. The two lists
// are read from the files the gates read, deny.toml (`[advisories] ignore`, for cargo deny, whose entries take no
// expiry: the date is written in the reason, as "review by YYYY-MM-DD") and osv-scanner.toml (`ignoreUntil`, which
// osv-scanner enforces itself: past it, the advisory fails its job again). OSV also reads the RustSec database, so
// every advisory deny.toml ignores is ignored by osv-scanner.toml too, with the same date; OSV may hold more (the
// npm packages, GitHub advisories missing from RustSec). A list that cannot be read whole is an error, never a pass.
// Run with: node scripts/check-exception-dates.mjs; its tests: node --test scripts/*.test.mjs
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const HORIZON_DAYS = 183;
const DATE = /(\d{4}-\d{2}-\d{2})/;

/** The entries of `[advisories] ignore` in deny.toml: `{ id, date }`, date null when the reason gives none. */
export function denyExceptions(text) {
  const section = text.split(/^\[advisories\]\s*$/m)[1]?.split(/^\[/m)[0];
  if (section === undefined) throw new Error('deny.toml: no [advisories] section');
  const list = section.match(/^ignore\s*=\s*\[([\s\S]*?)^\]/m);
  if (!list) return [];
  const body = list[1]
    .split('\n')
    .filter((line) => !line.trimStart().startsWith('#'))
    .join('\n');
  const entries = [...body.matchAll(/\{([^}]*)\}|"([^"]+)"/g)].map((m) => {
    if (m[2]) return { id: m[2], date: null, table: false };
    const id = m[1].match(/\bid\s*=\s*"([^"]+)"/)?.[1];
    const reason = m[1].match(/\breason\s*=\s*"([^"]*)"/)?.[1] ?? '';
    return { id, date: reason.match(new RegExp(`review by ${DATE.source}`))?.[1] ?? null, table: true };
  });
  // Every `id =` of the list belongs to one table read whole: a shape this reading does not know is refused.
  const tables = entries.filter((e) => e.table);
  if (tables.some((e) => !e.id) || tables.length !== (body.match(/\bid\s*=/g) ?? []).length) {
    throw new Error('deny.toml: an entry of [advisories] ignore could not be read');
  }
  return entries.map(({ id, date }) => ({ id, date }));
}

/** The `[[IgnoredVulns]]` entries of osv-scanner.toml: `{ id, date }`, date null without `ignoreUntil`. */
export function osvExceptions(text) {
  const blocks = text.split(/^\[\[IgnoredVulns\]\]\s*$/m).slice(1);
  return blocks.map((block) => {
    const own = block.split(/^\[/m)[0];
    const id = own.match(/^id\s*=\s*"([^"]+)"/m)?.[1];
    if (!id) throw new Error('osv-scanner.toml: an [[IgnoredVulns]] entry has no id');
    return { id, date: own.match(new RegExp(`^ignoreUntil\\s*=\\s*${DATE.source}`, 'm'))?.[1] ?? null };
  });
}

const dayNumber = (iso) => Math.floor(Date.parse(`${iso}T00:00:00Z`) / 86_400_000);

/** The problems of the two lists on `today` (an ISO date), with the number of exceptions checked. */
export function checkExceptions(denyText, osvText, today) {
  const problems = [];
  const lists = { 'deny.toml': denyExceptions(denyText), 'osv-scanner.toml': osvExceptions(osvText) };
  const now = dayNumber(today);
  for (const [file, entries] of Object.entries(lists)) {
    for (const { id, date } of entries) {
      if (!date) {
        problems.push(`${file}: ${id} has no date to be looked at again`);
      } else if (Number.isNaN(dayNumber(date))) {
        problems.push(`${file}: ${id} has a date that is not one (${date})`);
      } else if (dayNumber(date) <= now) {
        problems.push(`${file}: ${id} expired on ${date}: fix it, or look at it again and give a new date`);
      } else if (dayNumber(date) > now + HORIZON_DAYS) {
        problems.push(`${file}: ${id} is dated ${date}, more than ${HORIZON_DAYS} days ahead`);
      }
    }
  }
  const [deny, osv] = Object.values(lists);
  const dated = (entries) => new Map(entries.map((e) => [e.id, e.date]));
  const [denyDates, osvDates] = [dated(deny), dated(osv)];
  for (const id of denyDates.keys()) {
    if (!osvDates.has(id)) {
      problems.push(`${id} is ignored by deny.toml but not by osv-scanner.toml, which reads RustSec too`);
    } else if (denyDates.get(id) && osvDates.get(id) && denyDates.get(id) !== osvDates.get(id)) {
      problems.push(`${id}: deny.toml says ${denyDates.get(id)}, osv-scanner.toml ${osvDates.get(id)}`);
    }
  }
  return { problems, count: deny.length + osv.length };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
  const read = (name) => readFileSync(path.join(root, name), 'utf8');
  const today = new Date().toISOString().slice(0, 10);
  const { problems, count } = checkExceptions(read('deny.toml'), read('osv-scanner.toml'), today);
  for (const problem of problems) console.error(problem);
  if (problems.length > 0) process.exit(1);
  console.log(`${count} ignored advisories, dated within ${HORIZON_DAYS} days, those of deny.toml the same in OSV.`);
}
