#!/usr/bin/env node
// Fails when a comment or a test title of a covered file is written in French. Comments and test titles are in
// English, the language every reviewer and contributor shares; French belongs in the translation catalogues. Other
// strings are never reported, whatever their language: a sentence of the interface, a test fixture, example data.
//
// Each file is scanned with the comment and string syntax of its language (Rust, JavaScript, Svelte, CSS), so that a
// "//" inside a URL, a regular expression or a string is not taken for a comment. A test title is the first argument
// of test(), describe() or it() (with modifiers such as test.describe.skip), or the name of a scenario in a scenario
// file (JSON). A line or a title is French when it holds one of FRENCH_WORDS or an elision, outside code quoted with
// backticks.
// Usage: node scripts/check-french-comments.mjs [repository root] (defaults to this script's repository).
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Git pathspecs of the files whose comments and test titles must stay in English ("*" stays within a directory, "**"
// crosses them). A path joins the list once it is translated, so that it cannot slip back.
export const COVERED = [
  'src/**/*.rs',
  'tests/**/*.rs',
  'build.rs',
  'fuzz/**/*.rs',
  'scripts/*.mjs',
  'frontend/*.js',
  'frontend/e2e/**/*.js',
  'frontend/e2e/**/*.mjs',
  'frontend/e2e/scenarios/*.json',
  'frontend/src/main.js',
  'frontend/src/*.css',
  'frontend/src/App.svelte',
  'frontend/src/lib/*.js',
  'frontend/src/lib/components/*.svelte',
  'frontend/src/preview/*.js',
  'frontend/src/preview/*.svelte',
];

// Frequent French words that English comments do not use, with and without their accents (comments are often typed
// without them). Words that English shares are left out: "en" (a language code), "est" (a time zone), "par", "cas"
// (compare-and-swap), "aux", "sans", "tout", "encore". The last ones are nouns and verbs that test titles use: a
// title often goes without the articles that give a sentence away (`groupe: nom accentue accepte`).
export const FRENCH_WORDS = [
  'les',
  'pour',
  'avec',
  'une',
  'sont',
  'dans',
  'qui',
  'deja',
  'déjà',
  'regle',
  'règle',
  'requete',
  'requête',
  'meme',
  'même',
  'donc',
  'sinon',
  'aussi',
  'mais',
  'etre',
  'être',
  'cette',
  'cela',
  'lorsque',
  'puis',
  'chaque',
  'des',
  'du',
  'de',
  'le',
  'la',
  'un',
  'et',
  'ou',
  'au',
  'ce',
  'ces',
  'il',
  'ne',
  'pas',
  'si',
  'sur',
  'à',
  'où',
  'quand',
  'comme',
  'doit',
  'peut',
  'fait',
  'tous',
  'toujours',
  'jamais',
  'rien',
  'avant',
  'apres',
  'après',
  'selon',
  'entre',
  'ici',
  'voir',
  'cote',
  'côté',
  'plutot',
  'plutôt',
  'seul',
  'seule',
  'deux',
  'reste',
  'etat',
  'état',
  'defaut',
  'défaut',
  'parce',
  'aucun',
  'aucune',
  'avancee',
  'avancée',
  'avancees',
  'avancées',
  'comportement',
  'donnees',
  'données',
  'erreur',
  'exemple',
  'exemples',
  'fausse',
  'fonctionne',
  'groupe',
  'groupes',
  'pliage',
  'purement',
  'regles',
  'règles',
  'reponse',
  'réponse',
  'reponses',
  'réponses',
  'requetes',
  'requêtes',
  'reseau',
  'réseau',
];

// A letter, a digit, "_" or "-" next to a listed word makes it part of another word ("de-duplicate", "en-AU").
const WORD_CHAR = String.raw`[\p{L}\p{N}_-]`;
// An elided article or pronoun: `l'état`, `d'un`, `qu'il`, `n'est`.
const ELISION = String.raw`(?:[cdjlmnst]|qu)['’]\p{L}`;
const FRENCH = new RegExp(
  String.raw`(?<!${WORD_CHAR})(?:(?:${FRENCH_WORDS.join('|')})(?!${WORD_CHAR})|${ELISION})`,
  'iu',
);
// Code quoted in a comment (`de`, `la`) is not prose.
const QUOTED_CODE = /`[^`]*`/g;

const SYNTAX_BY_EXTENSION = {
  '.rs': 'rust',
  '.js': 'js',
  '.mjs': 'js',
  '.svelte': 'svelte',
  '.css': 'css',
  '.json': 'json',
};

// Functions whose first argument is a test title (Playwright, Vitest, node:test), and what may follow them before the
// opening parenthesis: modifiers (test.describe.skip) and blank space, up to the opening quote of the title.
const TEST_FUNCTIONS = new Set(['test', 'describe', 'it']);
const TITLE_CALL =
  /(?:\s*\.\s*(?:describe|only|skip|fixme|fail|slow|serial|parallel|step|todo|concurrent))*\s*\(\s*['"`]/y;
// A scenario name in a scenario file: "scenario": "…".
const SCENARIO_NAME = /"scenario"\s*:\s*("(?:\\.|[^"\\])*")/g;

// Words after which a "/" starts a regular expression rather than a division.
const KEYWORDS_BEFORE_EXPRESSION = new Set([
  'return',
  'typeof',
  'instanceof',
  'in',
  'of',
  'new',
  'delete',
  'void',
  'throw',
  'case',
  'do',
  'else',
  'yield',
  'await',
]);

const WORD = /[\p{L}\p{N}_$]+/uy;
const RUST_RAW_STRING = /b?r(#*)"/y;
const RUST_CHAR = /'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])'/uy;
// Lower case only, as Svelte reads them: a <SCRIPT> or a <Style> is a component, whose content is markup.
const SVELTE_EMBEDDED = /<(script|style)\b[^>]*>/y;

function matchAt(pattern, text, index) {
  pattern.lastIndex = index;
  return pattern.exec(text);
}

// Collects comments as one { line, text } per line they span, and test titles as one { line, text } each.
class Comments {
  constructor(text) {
    this.text = text;
    this.lines = [];
    this.titles = [];
    this.lineStarts = [0];
    for (let i = 0; i < text.length; i++) if (text[i] === '\n') this.lineStarts.push(i + 1);
  }

  lineOf(index) {
    let low = 0;
    let high = this.lineStarts.length - 1;
    while (low < high) {
      const middle = (low + high + 1) >> 1;
      if (this.lineStarts[middle] <= index) low = middle;
      else high = middle - 1;
    }
    return low + 1;
  }

  add(start, end) {
    const first = this.lineOf(start);
    this.text
      .slice(start, end)
      .split('\n')
      .forEach((text, offset) => {
        this.lines.push({ line: first + offset, text: text.replace(/\r$/, '') });
      });
  }

  // The title whose string literal opens at `start` (a quote or a backtick): escapes resolved, interpolations shown
  // as ${…}, since their content is code.
  addTitle(start) {
    const { text } = this;
    const quote = text[start];
    let title = '';
    let depth = 0;
    for (let i = start + 1; i < text.length; i++) {
      const c = text[i];
      if (depth > 0) {
        if (c === '{') depth++;
        else if (c === '}') depth--;
      } else if (c === '\\') {
        title += text[++i] ?? '';
      } else if (c === quote || (c === '\n' && quote !== '`')) {
        break;
      } else if (quote === '`' && c === '$' && text[i + 1] === '{') {
        title += '${…}';
        depth = 1;
        i++;
      } else {
        title += c;
      }
    }
    this.titles.push({ line: this.lineOf(start), text: title });
  }
}

function lineEnd(text, index, limit) {
  const end = text.indexOf('\n', index);
  return end === -1 || end > limit ? limit : end;
}

function after(text, index, limit, closing) {
  const end = text.indexOf(closing, index);
  return end === -1 || end + closing.length > limit ? limit : end + closing.length;
}

// A quoted string of JavaScript or CSS, which cannot span lines unescaped.
function skipQuoted(text, index, limit) {
  const quote = text[index];
  for (let i = index + 1; i < limit; i++) {
    if (text[i] === '\\') i++;
    else if (text[i] === quote) return i + 1;
    else if (text[i] === '\n') return i;
  }
  return limit;
}

// The text of a template literal from `index`, up to its closing backtick or to its next `${`.
function scanTemplateText(text, index, limit) {
  for (let i = index; i < limit; i++) {
    if (text[i] === '\\') i++;
    else if (text[i] === '`') return { index: i + 1, interpolation: false };
    else if (text[i] === '$' && text[i + 1] === '{') return { index: i + 2, interpolation: true };
  }
  return { index: limit, interpolation: false };
}

// The end of a regular expression literal starting at `index`, or -1 when the "/" cannot start one.
function regexEnd(text, index, limit) {
  let inClass = false;
  for (let i = index + 1; i < limit; i++) {
    const c = text[i];
    if (c === '\n') return -1;
    if (c === '\\') i++;
    else if (c === '[') inClass = true;
    else if (c === ']') inClass = false;
    else if (c === '/' && !inClass) {
      let end = i + 1;
      while (end < limit && /[a-z]/.test(text[end])) end++;
      return end;
    }
  }
  return -1;
}

// Scans JavaScript from `index`. Inside a Svelte expression (`inBraces`), stops after the "}" that closes it and
// returns the index that follows; otherwise scans up to `limit`.
function scanJs(comments, index, limit, inBraces) {
  const { text } = comments;
  let depth = 0;
  // Brace depth at which each open template literal entered a `${`.
  const templates = [];
  let regexAllowed = true;
  // The last character of code read, comments and blank space aside: a word after a "." is a property or a method.
  let previous = '';
  let i = index;
  const enterTemplate = (from) => {
    const part = scanTemplateText(text, from, limit);
    i = part.index;
    if (part.interpolation) {
      templates.push(depth);
      depth++;
    }
    regexAllowed = part.interpolation;
  };
  while (i < limit) {
    const c = text[i];
    const next = text[i + 1];
    if (c === '/' && next === '/') {
      const end = lineEnd(text, i, limit);
      comments.add(i, end);
      i = end;
      continue;
    } else if (c === '/' && next === '*') {
      const end = after(text, i + 2, limit, '*/');
      comments.add(i, end);
      i = end;
      continue;
    } else if (c === '"' || c === "'") {
      i = skipQuoted(text, i, limit);
      regexAllowed = false;
    } else if (c === '`') {
      enterTemplate(i + 1);
    } else if (c === '/') {
      const end = regexAllowed ? regexEnd(text, i, limit) : -1;
      i = end === -1 ? i + 1 : end;
      regexAllowed = end === -1;
    } else if (c === '{') {
      depth++;
      i++;
      regexAllowed = true;
    } else if (c === '}') {
      if (templates.length && templates[templates.length - 1] === depth - 1) {
        templates.pop();
        depth--;
        enterTemplate(i + 1);
      } else if (depth === 0 && inBraces) {
        return i + 1;
      } else {
        depth--;
        i++;
        regexAllowed = true;
      }
    } else if (matchAt(WORD, text, i)) {
      const end = WORD.lastIndex;
      const word = text.slice(i, end);
      // The title is a string the loop then skips like any other; only its position is taken here.
      if (TEST_FUNCTIONS.has(word) && previous !== '.' && matchAt(TITLE_CALL, text, end)) {
        comments.addTitle(TITLE_CALL.lastIndex - 1);
      }
      regexAllowed = KEYWORDS_BEFORE_EXPRESSION.has(word);
      i = end;
    } else {
      if (/\s/.test(c)) {
        i++;
        continue;
      }
      regexAllowed = c !== ')' && c !== ']';
      i++;
    }
    previous = text[i - 1];
  }
  return limit;
}

function scanCss(comments, index, limit) {
  const { text } = comments;
  let i = index;
  while (i < limit) {
    if (text[i] === '/' && text[i + 1] === '*') {
      const end = after(text, i + 2, limit, '*/');
      comments.add(i, end);
      i = end;
    } else if (text[i] === '"' || text[i] === "'") {
      i = skipQuoted(text, i, limit);
    } else {
      i++;
    }
  }
}

// Markup: HTML comments, then JavaScript in <script> and in `{…}` expressions, CSS in <style>. Text and attribute
// values are not code: an apostrophe there opens no string.
function scanSvelte(comments) {
  const { text } = comments;
  let i = 0;
  while (i < text.length) {
    const embedded = text[i] === '<' && matchAt(SVELTE_EMBEDDED, text, i);
    if (text.startsWith('<!--', i)) {
      const end = after(text, i + 4, text.length, '-->');
      comments.add(i, end);
      i = end;
    } else if (embedded) {
      const bodyStart = SVELTE_EMBEDDED.lastIndex;
      const close = text.indexOf(`</${embedded[1]}`, bodyStart);
      const bodyEnd = close === -1 ? text.length : close;
      if (embedded[1] === 'script') scanJs(comments, bodyStart, bodyEnd, false);
      else scanCss(comments, bodyStart, bodyEnd);
      i = bodyEnd;
    } else if (text[i] === '{' && text[i + 1] === '/') {
      // The end of a block ({/if}, {/each}…) holds no expression.
      i = after(text, i, text.length, '}');
    } else if (text[i] === '{') {
      i = scanJs(comments, i + (/[#:@]/.test(text[i + 1]) ? 2 : 1), text.length, true);
    } else {
      i++;
    }
  }
}

function scanRust(comments) {
  const { text } = comments;
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    const next = text[i + 1];
    const raw = matchAt(RUST_RAW_STRING, text, i);
    if (c === '/' && next === '/') {
      const end = lineEnd(text, i, text.length);
      comments.add(i, end);
      i = end;
    } else if (raw) {
      i = after(text, RUST_RAW_STRING.lastIndex, text.length, `"${raw[1]}`);
    } else if (c === '/' && next === '*') {
      // Block comments nest in Rust.
      let depth = 1;
      let end = i + 2;
      while (end < text.length && depth > 0) {
        if (text.startsWith('/*', end)) {
          depth++;
          end += 2;
        } else if (text.startsWith('*/', end)) {
          depth--;
          end += 2;
        } else {
          end++;
        }
      }
      comments.add(i, end);
      i = end;
    } else if (c === '"') {
      i++;
      while (i < text.length && text[i] !== '"') i += text[i] === '\\' ? 2 : 1;
      i++;
    } else if (c === "'") {
      // A character literal, or else the quote of a lifetime ('a, 'static).
      i = matchAt(RUST_CHAR, text, i) ? RUST_CHAR.lastIndex : i + 1;
    } else if (matchAt(WORD, text, i)) {
      // Whole identifiers, so that the "r" or "br" of a raw string is only looked for where a token starts.
      i = WORD.lastIndex;
    } else {
      i++;
    }
  }
}

// JSON has no comments; in a scenario file, the names of the scenarios are test titles.
function scanJson(comments) {
  for (const match of comments.text.matchAll(SCENARIO_NAME)) {
    comments.titles.push({ line: comments.lineOf(match.index), text: JSON.parse(match[1]) });
  }
}

function scan(text, syntax) {
  const comments = new Comments(text);
  if (syntax === 'rust') scanRust(comments);
  else if (syntax === 'js') scanJs(comments, 0, text.length, false);
  else if (syntax === 'svelte') scanSvelte(comments);
  else if (syntax === 'css') scanCss(comments, 0, text.length);
  else if (syntax === 'json') scanJson(comments);
  else throw new Error(`unknown syntax ${syntax}`);
  return comments;
}

/** The comment lines of `text`, written in `syntax` ('rust', 'js', 'svelte', 'css' or 'json'), as { line, text }. */
export function commentLines(text, syntax) {
  return scan(text, syntax).lines;
}

/** The test titles of `text`, written in `syntax`, as { line, text }. */
export function testTitles(text, syntax) {
  return scan(text, syntax).titles;
}

export function isFrench(commentText) {
  return FRENCH.test(commentText.replace(QUOTED_CODE, ''));
}

/**
 * Returns the French comment lines and test titles found in `files` (repository-relative paths with `/` separators),
 * as "file:line: comment" and "file:line: title "…""; `read(file)` returns the text of a file.
 */
export function checkFiles(files, read) {
  const problems = [];
  for (const file of files) {
    const syntax = SYNTAX_BY_EXTENSION[path.posix.extname(file)];
    if (!syntax) {
      problems.push(`${file}: no comment syntax known for this kind of file`);
      continue;
    }
    const { lines, titles } = scan(read(file), syntax);
    const found = [
      ...lines.filter(({ text }) => isFrench(text)).map(({ line, text }) => ({ line, problem: text.trim() })),
      ...titles.filter(({ text }) => isFrench(text)).map(({ line, text }) => ({ line, problem: `title "${text}"` })),
    ];
    found.sort((a, b) => a.line - b.line);
    problems.push(...found.map(({ line, problem }) => `${file}:${line}: ${problem}`));
  }
  return problems;
}

function main() {
  const root = path.resolve(process.argv[2] ?? path.join(path.dirname(fileURLToPath(import.meta.url)), '..'));
  // The files Git tracks, so that a local run checks exactly what CI checks.
  const pathspecs = COVERED.map((pattern) => `:(glob)${pattern}`);
  const files = execFileSync('git', ['ls-files', '--', ...pathspecs], { cwd: root, encoding: 'utf8' })
    .split('\n')
    .filter(Boolean);
  const problems = checkFiles(files, (file) => readFileSync(path.join(root, file), 'utf8'));
  if (problems.length) {
    console.error(problems.join('\n'));
    console.error(`\n${problems.length} French comment line(s) or test title(s): both are written in English.`);
    process.exit(1);
  }
  console.log(`The comments and test titles of ${files.length} covered files are in English.`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
