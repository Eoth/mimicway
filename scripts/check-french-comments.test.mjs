// Tests of check-french-comments.mjs: French comments are reported in every syntax, French strings never are, and the
// command fails on a French comment in each covered path. Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { COVERED, checkFiles, commentLines, isFrench, testTitles } from './check-french-comments.mjs';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'check-french-comments.mjs');

function check(files) {
  return checkFiles(Object.keys(files), (file) => files[file]);
}

test('English comments pass, a French line comment is reported with its line', () => {
  const js = '// Keeps the order of the rules.\nconst a = 1;\n// Garde les regles dans leur ordre.\n';
  assert.deepEqual(check({ 'a.js': '// Keeps the order of the rules.\n' }), []);
  assert.deepEqual(check({ 'a.js': js }), ['a.js:3: // Garde les regles dans leur ordre.']);
});

test('short French comments are recognised, with or without accents, by their words or their elisions', () => {
  const french = [
    "// Réinitialise l'état.",
    '// Valeur par defaut',
    '// Voir plus haut',
    '// Le serveur ne répond pas',
    "// qu'il soit vide",
  ];
  const js = french.join('\nx();\n');
  assert.deepEqual(
    check({ 'a.js': js }).map((p) => p.split(': ')[0]),
    ['a.js:1', 'a.js:3', 'a.js:5', 'a.js:7', 'a.js:9'],
  );
});

test('terse French, as test titles write it without articles, is recognised', () => {
  // Titles the end-to-end suite once had, which the articles and pronouns alone did not reveal.
  const titles = [
    'groupe: nom accentue accepte',
    'UI servie sans aucun service',
    'Service purement mocke : comportement reseau',
    'service: toggle mock/proxy fonctionne',
    'identite: suppression sans fausse erreur',
    'Runner (scenarios JSON) - lot 7 (pliage JSON + options avancees)',
    'Runner (scenarios JSON) - lot 10 (XML par exemple)',
    'Runner (scenarios JSON) - lot 16 (diagnostic reponse JSON/XML)',
  ];
  assert.deepEqual(
    titles.filter((title) => !isFrench(title)),
    [],
  );
});

test('English that shares letters with French words is not reported', () => {
  const english = [
    '// De-duplicate the en-AU and fr-CA entries.',
    "// It's the user's choice: don't retry, and don't tout it.",
    '// The `de` and `la` fields, `si` and `ou` flags.',
    '// A CAS loop, times in EST, on par with sans-serif fonts, an encore.',
  ].join('\n');
  assert.deepEqual(check({ 'a.js': english }), []);
});

test('strings, templates, regular expressions and URLs are not comments', () => {
  const js = [
    't("Les règles sont appliquées dans cet ordre");',
    'const url = \'http://example.com/les/regles\'; const b = "// pour";',
    'const tpl = `// une ligne ${t("dans une chaine")} // avec`;',
    'const re = /\\/\\/ avec|["\']/g; const half = total / 2 / 3; // two divisions',
    'const nested = `a ${`b // pour ${c}`} d`; // still code before this comment',
  ].join('\n');
  assert.deepEqual(check({ 'a.js': js }), []);
  assert.deepEqual(
    commentLines(js, 'js').map((c) => c.line),
    [4, 5],
  );
});

test('block comments report the line that holds French', () => {
  const js = '/**\n * Parses a template.\n * Retourne les champs du template.\n */\nexport function f() {}\n';
  assert.deepEqual(check({ 'a.js': js }), ['a.js:3: * Retourne les champs du template.']);
});

test('Svelte: HTML comments, script, expressions and style are read; markup text is not', () => {
  const svelte = [
    '<script>',
    '  const label = t("Les services"); // the label\'s text',
    '</script>',
    '<!-- Liste des services, avec leur groupe -->',
    '<p title="L\'URL des services">Les services sont ici</p>',
    '<button onclick={() => {',
    '  // Ouvre le formulaire pour un nouveau service',
    '  open = true;',
    '}}>{t("Ajouter")}</button>',
    '{#if open}<p>{label}</p>{/if}',
    '<style>',
    '  .a { content: "/* pour */"; } /* Marge pour les cartes */',
    '</style>',
  ].join('\n');
  assert.deepEqual(check({ 'A.svelte': svelte }), [
    'A.svelte:4: <!-- Liste des services, avec leur groupe -->',
    'A.svelte:7: // Ouvre le formulaire pour un nouveau service',
    'A.svelte:12: /* Marge pour les cartes */',
  ]);
});

test('Svelte: a <SCRIPT> or a <Style> is a component, as Svelte reads it, and its content is markup', () => {
  const svelte = [
    '<SCRIPT>// pour la suite</SCRIPT>',
    '<Style>/* pour plus tard */</Style>',
    '<script>',
    '  // Ouvre le formulaire',
    '</script>',
  ];
  assert.deepEqual(check({ 'A.svelte': svelte.join('\n') }), ['A.svelte:4: // Ouvre le formulaire']);
});

test('Svelte: a tag whose name only starts with script or style, like a custom element, is not a code block', () => {
  const svelte = [
    '<scripts>// pour la suite</scripts>',
    '<style-sheet>/* pour plus tard */</style-sheet>',
    '<script lang="ts">',
    '  // Ouvre le formulaire',
    '</script>',
  ];
  assert.deepEqual(check({ 'A.svelte': svelte.join('\n') }), ['A.svelte:4: // Ouvre le formulaire']);
});

test('Rust: strings, raw strings, character literals and lifetimes do not hide or fake a comment', () => {
  const rust = [
    "fn f<'a>(s: &'a str) -> char { '\"' } // returns a quote",
    'const A: &str = "// pour les tests";',
    'const B: &str = r#"une " chaine // avec"#; // English',
    "const C: u8 = b'\\''; // puis la suite",
    '/* outer /* inner */ toujours dans le commentaire, avec des mots */',
  ].join('\n');
  assert.deepEqual(check({ 'a.rs': rust }), [
    'a.rs:4: // puis la suite',
    'a.rs:5: /* outer /* inner */ toujours dans le commentaire, avec des mots */',
  ]);
});

test('test titles are read from test(), describe() and it(), with their modifiers', () => {
  const js = [
    "test('Garde les règles dans leur ordre', () => {});",
    "test.describe('The services', () => {",
    '  describe.skip("pour plus tard", () => {});',
    '  it(`ne répond pas à ${name} ici`, () => {});',
    '});',
    'test.describe.serial(',
    "  'Les groupes',",
    '  () => {},',
    ');',
    '// A comment that ends with a period.',
    "test('Après un commentaire', () => {});",
  ].join('\n');
  assert.deepEqual(
    testTitles(js, 'js').map(({ line, text }) => [line, text]),
    [
      [1, 'Garde les règles dans leur ordre'],
      [2, 'The services'],
      [3, 'pour plus tard'],
      [4, 'ne répond pas à ${…} ici'],
      [7, 'Les groupes'],
      [11, 'Après un commentaire'],
    ],
  );
});

test('a method named test, a string or a comment holds no title', () => {
  const js = [
    "/les/.test('les règles');",
    "pattern?.test('les règles');",
    'const s = "test(\'les règles\')";',
    "// test('the rules') in a comment",
    "const t = `test('les règles')`;",
  ].join('\n');
  assert.deepEqual(testTitles(js, 'js'), []);
});

test('a French test title is reported with its line, an English one passes', () => {
  const js = "test('keeps the order of the rules', () => {});\ntest('garde l\\'ordre des règles', () => {});\n";
  assert.deepEqual(check({ 'a.spec.js': js }), ['a.spec.js:2: title "garde l\'ordre des règles"']);
});

test('the names of the scenarios of a scenario file are titles, their steps are data', () => {
  const json = [
    '{',
    '  "domain": "rules",',
    '  "scenarios": [',
    '    { "scenario": "Create a rule", "steps": [] },',
    '    { "scenario": "Créer une règle", "steps": [{ "action": "fill", "value": "une valeur" }] }',
    '  ]',
    '}',
  ].join('\n');
  assert.deepEqual(check({ 'rules.scenarios.json': json }), ['rules.scenarios.json:5: title "Créer une règle"']);
});

test('a file in an unknown syntax is reported rather than skipped', () => {
  assert.deepEqual(check({ 'a.py': '# pour' }), ['a.py: no comment syntax known for this kind of file']);
});

// A file name that each covered pathspec matches.
function sampleOf(pattern) {
  return pattern.replaceAll('**/', '').replaceAll('*', 'sample');
}

// A file of comment lines, written in the comment syntax of `file`: in Svelte markup and in CSS, "//" is not a comment.
// JSON has no comments: a scenario file gets one scenario per line, named after it.
function commented(file, ...lines) {
  const extension = path.posix.extname(file);
  if (extension === '.json') {
    return `{"scenarios": [${lines.map((line) => `{"scenario": ${JSON.stringify(line)}}`).join(',\n')}]}\n`;
  }
  const [open, close] = { '.svelte': ['<!-- ', ' -->'], '.css': ['/* ', ' */'] }[extension] ?? ['// ', ''];
  return lines.map((line) => `${open}${line}${close}\n`).join('');
}

// A file of tests, one per line, named after the lines.
function titled(...lines) {
  return lines.map((line) => `test(${JSON.stringify(line)}, () => {});\n`).join('');
}

// Files that must be checked, whatever the patterns of COVERED: the end-to-end suite, its scenarios and the tooling
// configuration of the interface.
const MUST_BE_COVERED = [
  'frontend/e2e/login.spec.js',
  'frontend/e2e/config.spec.mjs',
  'frontend/e2e/scenario-runner.js',
  'frontend/e2e/scenarios/home.scenarios.json',
  'frontend/vite.config.js',
];

test('the command fails on a French comment or test title in every covered path, and ignores the others', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'french-comments-'));
  try {
    const probes = [...new Set([...COVERED.map(sampleOf), ...MUST_BE_COVERED])];
    const files = Object.fromEntries(probes.map((file) => [file, commented(file, 'The first line is fine.')]));
    files['notes/sample.js'] = '// Pas encore traduit, pour plus tard.\n';
    for (const [file, text] of Object.entries(files)) {
      mkdirSync(path.join(root, path.dirname(file)), { recursive: true });
      writeFileSync(path.join(root, file), text);
    }
    execFileSync('git', ['init', '-q'], { cwd: root });
    execFileSync('git', ['add', '.'], { cwd: root });
    const clean = spawnSync(process.execPath, [SCRIPT, root], { encoding: 'utf8' });
    assert.equal(clean.status, 0, clean.stderr);

    for (const file of probes) {
      writeFileSync(path.join(root, file), commented(file, 'The first line is fine.', 'Mais pas la seconde.'));
      const broken = spawnSync(process.execPath, [SCRIPT, root], { encoding: 'utf8' });
      assert.equal(broken.status, 1, `${file}: ${broken.stdout}`);
      assert.match(broken.stderr, new RegExp(`^${file.replaceAll('.', '\\.')}:2: .*Mais pas la seconde\\.`, 'm'));
      writeFileSync(path.join(root, file), commented(file, 'The first line is fine.'));
    }

    for (const file of probes.filter((probe) => /\.m?js$/.test(probe))) {
      writeFileSync(path.join(root, file), titled('The first test is fine.', 'Mais pas le second.'));
      const broken = spawnSync(process.execPath, [SCRIPT, root], { encoding: 'utf8' });
      assert.equal(broken.status, 1, `${file}: ${broken.stdout}`);
      assert.match(broken.stderr, new RegExp(`^${file.replaceAll('.', '\\.')}:2: title "Mais pas le second\\."`, 'm'));
      writeFileSync(path.join(root, file), commented(file, 'The first line is fine.'));
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
