// The interface's packages install without running their install scripts on a contributor's machine: npm itself,
// asked from frontend/ as a contributor runs it, must read ignore-scripts as true, whatever the user's own settings
// hold (an environment variable would override the file, so the test clears it). The lock file's packages that declare
// such a script are listed by the test, so that a new one shows up in its output.
// Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const frontend = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'frontend');

test('npm runs no install script in frontend/', (t) => {
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !/^npm_config_ignore_scripts$/i.test(k)));
  const value = execFileSync('npm', ['config', 'get', 'ignore-scripts'], { cwd: frontend, env, encoding: 'utf8' });
  assert.equal(value.trim(), 'true');
  const lock = JSON.parse(readFileSync(path.join(frontend, 'package-lock.json'), 'utf8'));
  const declared = Object.entries(lock.packages)
    .filter(([, p]) => p.hasInstallScript)
    .map(([name]) => name.replace(/^node_modules\//, ''));
  t.diagnostic(`packages of the lock file with an install script, not run: ${declared.join(', ') || 'none'}`);
});
