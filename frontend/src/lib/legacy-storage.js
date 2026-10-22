/**
 * Browser keys written under the project's former name (lightMock). Moved once to the current keys, before anything
 * reads them, so that a user keeps their language and theme after the rename.
 */
export const RENAMED_KEYS = [
  ['lightmock-locale', 'mimicway-locale'],
  ['lightmock-theme', 'mimicway-theme'],
];

/**
 * Keys where earlier versions kept the session, access and refresh tokens included. The session now lives in memory
 * (auth.svelte.js): a token left there would stay readable by any script of the origin, long after its use.
 */
export const SESSION_KEYS = ['lightmock-auth', 'mimicway-auth'];

/**
 * Copies each former key to its new name when the new one is not set yet, then removes the former key; deletes the
 * sessions that earlier versions saved.
 */
export function migrateLegacyStorage(storage = globalThis.localStorage) {
  try {
    for (const [legacy, current] of RENAMED_KEYS) {
      const value = storage.getItem(legacy);
      if (value === null) continue;
      if (storage.getItem(current) === null) storage.setItem(current, value);
      storage.removeItem(legacy);
    }
    for (const key of SESSION_KEYS) storage.removeItem(key);
  } catch {
    // Storage unavailable (private mode, sandboxed frame): nothing to move.
  }
}
