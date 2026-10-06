// The theme the interface opens in (lib/theme.js): the saved choice, else the system's preference.
import { afterEach, describe, expect, test, vi } from 'vitest';
import { THEME_KEY, applyTheme, initialTheme, saveTheme } from '../lib/theme.js';

const prefersDark = (dark) => {
  window.matchMedia = vi.fn().mockReturnValue({ matches: dark });
};

describe('initialTheme', () => {
  afterEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  test('a saved choice wins over the system preference', () => {
    prefersDark(true);
    localStorage.setItem(THEME_KEY, 'light');
    expect(initialTheme()).toBe('light');
    prefersDark(false);
    localStorage.setItem(THEME_KEY, 'dark');
    expect(initialTheme()).toBe('dark');
  });

  test('without a valid saved choice, the system preference applies', () => {
    prefersDark(true);
    expect(initialTheme()).toBe('dark');
    localStorage.setItem(THEME_KEY, 'sepia');
    expect(initialTheme()).toBe('dark');
    prefersDark(false);
    expect(initialTheme()).toBe('light');
  });

  test('blocked storage falls back to the system preference, and saving does not throw', () => {
    prefersDark(true);
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(initialTheme()).toBe('dark');
    expect(() => saveTheme('light')).not.toThrow();
  });

  test('applyTheme sets the attribute the tokens are keyed on', () => {
    applyTheme('dark');
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    applyTheme('light');
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
  });
});
