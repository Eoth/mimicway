import { describe, it, expect, beforeEach } from 'vitest';
import { auth, setAuth, logout, isLoggedIn } from '../lib/auth.svelte.js';

// The session lives in memory: a script of the page's origin finds no token in the browser's storage.
describe('auth: the session is kept in memory only', () => {
  beforeEach(() => {
    logout();
    localStorage.clear();
    sessionStorage.clear();
    auth.enabled = true;
  });

  it('signs in without writing anything to the browser storage', () => {
    setAuth({ access_token: 'access', refresh_token: 'refresh', username: 'alice', is_super_admin: true });

    expect(isLoggedIn()).toBe(true);
    expect(auth.token).toBe('access');
    expect(auth.username).toBe('alice');
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
  });

  it('keeps no refresh token, which nothing uses', () => {
    setAuth({ access_token: 'access', refresh_token: 'refresh', username: 'alice', is_super_admin: false });

    expect(JSON.stringify(auth)).not.toContain('refresh');
  });

  it('forgets the session on sign-out', () => {
    setAuth({ access_token: 'access', username: 'alice', is_super_admin: true });
    logout();

    expect(isLoggedIn()).toBe(false);
    expect(auth.token).toBeNull();
    expect(auth.isSuperAdmin).toBe(false);
  });
});
