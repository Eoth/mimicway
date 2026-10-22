import { mount } from 'svelte';
import App from './App.svelte';
import { loadRuntimeConfig } from './lib/runtime-config.js';
import { initLocale } from './lib/i18n.svelte.js';
import { migrateLegacyStorage } from './lib/legacy-storage.js';
import { applyTheme, initialTheme } from './lib/theme.js';
import './tokens.css';
import './app.css';

// The runtime configuration (base URL of the API, lib/runtime-config.js) is loaded before the app is mounted: the API
// calls the mount makes (GET /api/auth/status...) need the right target. An async function rather than a top-level
// await, which Vite's default build target (Chrome 87, Firefox 78, Safari 14) does not support: the build would fail.
(async () => {
  // Before anything reads the saved language or theme.
  migrateLegacyStorage();
  // The theme first, so that the page does not show light while the configuration and the language load.
  applyTheme(initialTheme());
  await Promise.all([loadRuntimeConfig(), initLocale()]);
  mount(App, { target: document.getElementById('app') });
})();
