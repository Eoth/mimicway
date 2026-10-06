// Tests of check-doc-translations.mjs: a complete pair passes, and each kind of drift fails, both through the
// function and through the command on a real Git repository. Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkTranslations } from './check-doc-translations.mjs';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'check-doc-translations.mjs');

// A repository whose English and French documentation match.
function completeRepository() {
  return {
    'README.md': '[Français](README.fr.md)\n\n# Tool\n\n![Home](docs/en/screenshots/home.png)\n\n## Start\n',
    'README.fr.md': '[English](README.md)\n\n# Outil\n\n![Accueil](docs/fr/screenshots/home.png)\n\n## Démarrer\n',
    'docs/en/index.md':
      '[Français](../fr/index.md)\n\n# Guide\n\n![Home](screenshots/home.png)\n\n## Rules\n\n### Order\n',
    'docs/fr/index.md':
      '[English](../en/index.md)\n\n# Guide\n\n![Accueil](screenshots/home.png)\n\n## Règles\n\n### Ordre\n',
    'docs/en/screenshots/home.png': '',
    'docs/fr/screenshots/home.png': '',
  };
}

function check(files) {
  return checkTranslations(Object.keys(files), (file) => files[file]);
}

test('matching translations pass', () => {
  assert.deepEqual(check(completeRepository()), []);
});

test('a page that exists in one language only fails', () => {
  const files = { ...completeRepository(), 'docs/fr/extra.md': '[English](../en/extra.md)\n\n# En plus\n' };
  const problems = check(files);
  assert.ok(
    problems.some((p) => p.startsWith('docs/en/extra.md: missing')),
    problems.join('\n'),
  );
});

test('a screenshot missing from one language fails', () => {
  const files = completeRepository();
  delete files['docs/fr/screenshots/home.png'];
  const problems = check(files);
  assert.ok(
    problems.some((p) => p.startsWith('docs/fr/screenshots/home.png: missing')),
    problems.join('\n'),
  );
});

test('a different number of headings at one level fails', () => {
  const files = completeRepository();
  files['docs/fr/index.md'] += '\n## Une section de plus\n';
  const problems = check(files);
  assert.ok(
    problems.some((p) => p.startsWith('docs/fr/index.md: headings differ') && p.includes('h2×2')),
    problems.join('\n'),
  );
});

test('headings in another order fail, headings inside code blocks do not count', () => {
  const files = completeRepository();
  files['docs/fr/index.md'] =
    '[English](../en/index.md)\n\n# Guide\n\n![Accueil](screenshots/home.png)\n\n### Ordre\n\n## Règles\n';
  assert.ok(check(files).some((p) => p.includes('in another order')));
  const withCode = completeRepository();
  withCode['docs/fr/index.md'] += '\n```bash\n# a shell comment, not a heading\n```\n';
  assert.deepEqual(check(withCode), []);
});

test('a page that shows other images fails', () => {
  const files = completeRepository();
  files['docs/fr/index.md'] = files['docs/fr/index.md'].replace('screenshots/home.png', 'screenshots/other.png');
  const problems = check(files);
  assert.ok(
    problems.some((p) => p.startsWith('docs/fr/index.md: images differ') && p.includes('missing screenshots/home.png')),
  );
});

test("a README showing the other language's images fails", () => {
  const files = completeRepository();
  files['README.fr.md'] = files['README.fr.md'].replace('docs/fr/', 'docs/en/');
  assert.ok(check(files).some((p) => p.startsWith('README.fr.md: images differ')));
});

test('a missing link to the other language on the first line fails', () => {
  const files = completeRepository();
  files['docs/en/index.md'] = files['docs/en/index.md'].replace('[Français](../fr/index.md)\n\n', '');
  files['README.md'] = '# Tool\n\n[Français](README.fr.md)\n\n![Home](docs/en/screenshots/home.png)\n\n## Start\n';
  const problems = check(files);
  assert.ok(problems.includes('docs/en/index.md: the first line does not link to the fr version (../fr/index.md)'));
  assert.ok(problems.includes('README.md: the first line does not link to the fr version (README.fr.md)'));
});

test('a guide language without its README, and a file outside the language directories, fail', () => {
  const files = completeRepository();
  delete files['README.fr.md'];
  files['docs/notes.md'] = '# Notes\n';
  const problems = check(files);
  assert.ok(problems.some((p) => p.startsWith('README.fr.md: missing')));
  assert.ok(problems.some((p) => p.startsWith('docs/notes.md: outside a language directory')));
});

test('the command fails on a repository with an orphan page and a missing image, and passes once fixed', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'doc-translations-'));
  try {
    const files = completeRepository();
    files['docs/en/extra.md'] = '[Français](../fr/extra.md)\n\n# Extra\n';
    delete files['docs/fr/screenshots/home.png'];
    for (const [file, text] of Object.entries(files)) {
      mkdirSync(path.join(root, path.dirname(file)), { recursive: true });
      writeFileSync(path.join(root, file), text);
    }
    execFileSync('git', ['init', '-q'], { cwd: root });
    execFileSync('git', ['add', '.'], { cwd: root });

    const broken = spawnSync(process.execPath, [SCRIPT, root], { encoding: 'utf8' });
    assert.equal(broken.status, 1);
    assert.match(broken.stderr, /docs\/fr\/extra\.md: missing/);
    assert.match(broken.stderr, /docs\/fr\/screenshots\/home\.png: missing/);

    writeFileSync(path.join(root, 'docs/fr/extra.md'), '[English](../en/extra.md)\n\n# En plus\n');
    mkdirSync(path.join(root, 'docs/fr/screenshots'), { recursive: true });
    writeFileSync(path.join(root, 'docs/fr/screenshots/home.png'), '');
    execFileSync('git', ['add', '.'], { cwd: root });
    const fixed = spawnSync(process.execPath, [SCRIPT, root], { encoding: 'utf8' });
    assert.equal(fixed.status, 0, fixed.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
