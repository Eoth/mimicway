import { describe, it, expect, beforeEach } from 'vitest';
import { migrateLegacyStorage, RENAMED_KEYS, SESSION_KEYS } from '../lib/legacy-storage.js';

describe('migrateLegacyStorage', () => {
  beforeEach(() => localStorage.clear());

  it('moves the language and theme saved under the former name', () => {
    localStorage.setItem('lightmock-locale', 'fr');
    localStorage.setItem('lightmock-theme', 'dark');

    migrateLegacyStorage();

    expect(localStorage.getItem('mimicway-locale')).toBe('fr');
    expect(localStorage.getItem('mimicway-theme')).toBe('dark');
    for (const [legacy] of RENAMED_KEYS) expect(localStorage.getItem(legacy)).toBeNull();
  });

  it('deletes the sessions, tokens included, that earlier versions saved under either name', () => {
    localStorage.setItem('lightmock-auth', '{"token":"t","refreshToken":"r","username":"alice"}');
    localStorage.setItem('mimicway-auth', '{"token":"t","refreshToken":"r","username":"bob"}');

    migrateLegacyStorage();

    expect(SESSION_KEYS).toEqual(['lightmock-auth', 'mimicway-auth']);
    expect(localStorage.length).toBe(0);
  });

  it('keeps a value already saved under the new name', () => {
    localStorage.setItem('lightmock-locale', 'fr');
    localStorage.setItem('mimicway-locale', 'en');

    migrateLegacyStorage();

    expect(localStorage.getItem('mimicway-locale')).toBe('en');
    expect(localStorage.getItem('lightmock-locale')).toBeNull();
  });

  it('does nothing when there is nothing to move', () => {
    migrateLegacyStorage();
    expect(localStorage.length).toBe(0);
  });

  it('survives a storage that refuses access', () => {
    const refusing = {
      getItem() {
        throw new Error('denied');
      },
    };
    expect(() => migrateLegacyStorage(refusing)).not.toThrow();
  });
});
