// The end-to-end suite resets the configuration of the server it reaches. It runs only against a server started for
// that, which says so in /api/health (DISPOSABLE_DATA=true), and with DATA_PATH set to the directory that server
// writes, which some specs read back: an absolute path outside the repository, never a directory where someone keeps
// mocks. Both are checked before the first test, so before any request that writes.
import { isAbsolute, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPOSITORY = fileURLToPath(new URL('../../', import.meta.url));
const HOW = 'Start one for the tests as frontend/e2e/README.md shows.';

/** The data directory of the server under test, from DATA_PATH, refused when relative or inside the repository. */
export function dataDir(env = process.env) {
  const path = env.DATA_PATH;
  if (!path) throw new Error(`DATA_PATH is not set: the suite reads back the files of the server it tests. ${HOW}`);
  // Relative, it would name another directory for the server (started at the root) and for Playwright (in frontend/).
  if (!isAbsolute(path)) throw new Error(`DATA_PATH must be an absolute path, got "${path}". ${HOW}`);
  const inside = relative(REPOSITORY, resolve(path));
  if (!inside.startsWith('..') && !isAbsolute(inside)) {
    throw new Error(`DATA_PATH "${path}" is inside the repository, where mocks of your own may live. ${HOW}`);
  }
  return resolve(path);
}

/** Resolves when the server at `baseURL` reports disposable data and DATA_PATH is usable; throws otherwise. */
export async function assertDisposableServer(baseURL, { fetch = globalThis.fetch, env = process.env } = {}) {
  let health;
  try {
    const res = await fetch(new URL('/api/health', baseURL));
    health = res.ok ? await res.json() : null;
  } catch (e) {
    throw new Error(`No Mimicway answers at ${baseURL} (${e.message}). ${HOW}`);
  }
  if (health?.disposable_data !== true) {
    throw new Error(
      `The Mimicway at ${baseURL} was not started with DISPOSABLE_DATA=true. The suite resets its configuration, ` +
        `so it refuses to run against it. ${HOW}`,
    );
  }
  dataDir(env);
}
