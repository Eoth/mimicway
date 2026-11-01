import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  // Refuses a server that was not started for tests: the suite resets its configuration.
  globalSetup: './e2e/global-setup.mjs',
  timeout: 15000,
  workers: 1,
  use: {
    baseURL: 'http://localhost:7342',
    headless: true,
    // Tests read the interface in its source language; i18n.spec.js covers the French one.
    locale: 'en-US',
  },
});
