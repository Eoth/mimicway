// Tests of check-former-design-system.mjs: a listed name is found whatever its case and separators, in a file's content
// or path and on standard input, binary files are skipped, and the command fails on it.
// Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { DIGESTS, checkFiles, findNames, words } from './check-former-design-system.mjs';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'check-former-design-system.mjs');

// A listed name, recovered from its digest by trying every word of three letters: spelling it here would put it in the
// repository. The shortest name has three letters; if the list changes, this says so rather than passing silently.
const NAME = (() => {
  const letters = 'abcdefghijklmnopqrstuvwxyz';
  for (const a of letters) {
    for (const b of letters) {
      for (const c of letters) {
        const word = a + b + c;
        if (DIGESTS.has(createHash('sha256').update(word).digest('hex'))) return word;
      }
    }
  }
  throw new Error('no listed name of three letters: write the test fixture another way');
})();
const CAPITALIZED = NAME[0].toUpperCase() + NAME.slice(1);

function check(files) {
  return checkFiles(Object.keys(files), (file) => Buffer.from(files[file]));
}

test('words are split on case, digits and every separator, and lowercased', () => {
  assert.deepEqual(words('--color-primary-500 tokens_Theme fooBar XMLHttp v2'), [
    'color',
    'primary',
    '500',
    'tokens',
    'theme',
    'foo',
    'bar',
    'xml',
    'http',
    'v',
    '2',
  ]);
});

test('a listed name is found whatever its case and separators, with its line and column', () => {
  const text = [
    'clean line',
    `  --${NAME}-color-primary-500: x;`,
    `tokens_${NAME.toUpperCase()} and ${CAPITALIZED}Theme`,
    `${NAME}2`,
  ].join('\n');
  assert.deepEqual(findNames(text), ['2:5', '3:8', '3:16', '4:1']);
});

test('a word that only contains a listed name is not reported', () => {
  assert.deepEqual(findNames(`${NAME}x x${NAME} ${NAME}ary`), []);
});

test('a path is checked like a content, and binary files are skipped', () => {
  assert.deepEqual(check({ 'src/app.css': 'body {}\n' }), []);
  assert.deepEqual(check({ [`src/tokens-${NAME}.css`]: 'body {}\n' }), [`src/tokens-${NAME}.css (in its path)`]);
  assert.deepEqual(check({ 'src/app.css': `a {}\n/* ${CAPITALIZED} */\n` }), ['src/app.css:2:4']);
  assert.deepEqual(check({ 'image.png': `\u0000${NAME}` }), []);
});

test('the command fails on a listed name in a tracked file or on standard input, and passes otherwise', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'former-design-system-'));
  try {
    mkdirSync(path.join(root, 'src'));
    writeFileSync(path.join(root, 'src/app.css'), ':root { --color-bg: white; }\n');
    execFileSync('git', ['init', '-q'], { cwd: root });
    execFileSync('git', ['add', '.'], { cwd: root });
    const clean = spawnSync(process.execPath, [SCRIPT, root], { encoding: 'utf8' });
    assert.equal(clean.status, 0, clean.stderr);

    writeFileSync(path.join(root, 'src/app.css'), `:root { --${NAME}-color-bg: white; }\n`);
    const broken = spawnSync(process.execPath, [SCRIPT, root], { encoding: 'utf8' });
    assert.equal(broken.status, 1, broken.stdout);
    assert.match(broken.stderr, /^src\/app\.css:1:11$/m);
    assert.doesNotMatch(broken.stderr, new RegExp(NAME, 'i'));

    const message = spawnSync(process.execPath, [SCRIPT, '--stdin'], {
      input: `fix: drop the ${CAPITALIZED} tokens\n`,
    });
    assert.equal(message.status, 1);
    const fine = spawnSync(process.execPath, [SCRIPT, '--stdin'], { input: 'fix: drop the former tokens\n' });
    assert.equal(fine.status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
