// The theme of the interface: the one chosen with the theme switch, else the system's preference. main.js applies it
// before the application fetches anything, so that a dark page does not show light while it loads; App.svelte starts
// from the same value and keeps the choice.
export const THEME_KEY = 'mimicway-theme';

export function initialTheme() {
  try {
    const saved = localStorage.getItem(THEME_KEY);
    if (saved === 'dark' || saved === 'light') return saved;
  } catch {
    // Storage blocked (private mode, policy): the system's preference still applies.
  }
  return typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light';
}

export function applyTheme(theme) {
  document.documentElement.setAttribute('data-theme', theme);
}

export function saveTheme(theme) {
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // Not saved: the theme holds until the page is reloaded.
  }
}
