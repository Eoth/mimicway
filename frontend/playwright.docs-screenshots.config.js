// Playwright configuration that regenerates the screenshots of docs/. Kept apart from playwright.config.js (used by
// `npm run test:e2e`): DOCS_SCREENSHOTS is set here, in the configuration module that Node runs before loading any
// test file, so it works the same from PowerShell, cmd or bash, and the standard run never captures anything
// (docs-screenshot.js only frames the subject until this variable is set).
//
// Only the files that take screenshots run: those calling docsScreenshot() and those replaying scenarios, whose
// "screenshot" steps call it. They are found by reading the files, so a new one cannot be forgotten in a list.
import fs from 'node:fs';
import { defineConfig } from '@playwright/test';

process.env.DOCS_SCREENSHOTS = '1';

const e2eDir = new URL('./e2e/', import.meta.url);
const takesScreenshots = (name) =>
  /\.spec\.m?js$/.test(name) && /\b(docsScreenshot|runScenario)\(/.test(fs.readFileSync(new URL(name, e2eDir), 'utf8'));

export default defineConfig({
  testDir: './e2e',
  testMatch: fs.readdirSync(e2eDir).filter(takesScreenshots),
  globalSetup: './e2e/global-setup.mjs',
  timeout: 15000,
  workers: 1,
  use: {
    baseURL: 'http://localhost:7342',
    headless: true,
    // Screenshots show the interface in its source language, like the documentation.
    locale: 'en-US',
  },
});
