// Holds the translations to the rule that keeps them cheap: every sentence exists once, in English, where it is
// used; a language adds a catalogue (src/locales/<locale>.json) and nothing else.
//
// What it checks:
//   * every message given to t / tCount is a string literal, single- or double-quoted (or such literals joined by
//     +), so it can be extracted: a template string or a variable would be a sentence no catalogue can know;
//   * each catalogue translates exactly the existing messages (nothing missing, nothing left over from a sentence
//     that was reworded) and keeps their {0}, {1}… placeholders;
//   * no visible word of the interface escapes t: the components are rendered in a pseudo-locale where every
//     translated text is marked, and any other word found in the page fails the test, as does punctuation spaced
//     the French way (" : ").
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { render, cleanup } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeAll, afterAll, afterEach } from 'vitest';
import { format, useTranslator, setLocale } from '../lib/i18n.svelte.js';
import * as api from '../lib/api.js';
import { SCREEN_GROUPS, answerApi, visibleTexts } from './helpers/screens.js';

const SRC = join(__dirname, '..');

// ---------------------------------------------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------------------------------------------

function sourceFiles(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === 'tests' || name === 'locales' ? [] : sourceFiles(path);
    return /\.(svelte|js)$/.test(name) && name !== 'i18n.svelte.js' ? [path] : [];
  });
}

/** Splits the arguments of the call whose opening parenthesis is at `open`. */
function argumentsOf(source, open) {
  const args = [];
  let depth = 0;
  let start = open + 1;
  for (let i = open + 1; i < source.length; i += 1) {
    const c = source[i];
    if (c === '"' || c === "'" || c === '`') {
      let j = i + 1;
      while (j < source.length && source[j] !== c) j += source[j] === '\\' ? 2 : 1;
      i = j;
    } else if ('([{'.includes(c)) {
      depth += 1;
    } else if (')]}'.includes(c)) {
      if (depth === 0) {
        args.push(source.slice(start, i));
        return args;
      }
      depth -= 1;
    } else if (c === ',' && depth === 0) {
      args.push(source.slice(start, i));
      start = i + 1;
    }
  }
  return args;
}

/** The value of a string literal's body written between `quote`s; a single-quoted one is read as its JSON twin. */
function literalValue(quote, body) {
  if (quote === '"') return JSON.parse(`"${body}"`);
  let twin = '';
  for (let k = 0; k < body.length; k += 1) {
    if (body[k] === '\\') {
      twin += body[k + 1] === "'" ? "'" : body.slice(k, k + 2);
      k += 1;
    } else {
      twin += body[k] === '"' ? '\\"' : body[k];
    }
  }
  return JSON.parse(`"${twin}"`);
}

/** The message of a literal argument (quoted literals joined by +), or undefined for anything else. */
function messageOf(argument) {
  const text = argument.trim();
  let value = '';
  let i = 0;
  while (i < text.length) {
    const quote = text[i];
    if (quote !== '"' && quote !== "'") return undefined;
    let j = i + 1;
    while (j < text.length && text[j] !== quote) j += text[j] === '\\' ? 2 : 1;
    value += literalValue(quote, text.slice(i + 1, j));
    const rest = text.slice(j + 1).match(/^\s*(\+\s*)?/);
    i = j + 1 + rest[0].length;
    if (rest[1] === undefined && i < text.length) return undefined;
  }
  return value;
}

const MESSAGE_ARGUMENTS = { t: [0], tCount: [1, 2] };
const messages = new Map();
const unextractable = [];
for (const file of sourceFiles(SRC)) {
  const text = readFileSync(file, 'utf8');
  const call = /(^|[^.\w$])(tCount|t)\(/g;
  let match;
  while ((match = call.exec(text)) !== null) {
    const open = match.index + match[0].length - 1;
    const args = argumentsOf(text, open);
    const line = text.slice(0, match.index).split('\n').length;
    for (const index of MESSAGE_ARGUMENTS[match[2]]) {
      const message = args[index] === undefined ? undefined : messageOf(args[index]);
      if (message === undefined) {
        unextractable.push(`${relative(SRC, file)}:${line} ${match[2]}(${(args[index] ?? '').trim().slice(0, 50)})`);
      } else if (!messages.has(message)) {
        messages.set(message, `${relative(SRC, file)}:${line}`);
      }
    }
  }
}

const placeholders = (text) => (text.match(/\{\d+\}/g) ?? []).sort().join(' ');

const catalogues = Object.fromEntries(
  readdirSync(join(SRC, 'locales'))
    .filter((name) => name.endsWith('.json'))
    .map((name) => [name, JSON.parse(readFileSync(join(SRC, 'locales', name), 'utf8'))]),
);

describe('translations', () => {
  it('extracts every message as a literal', () => {
    expect(unextractable).toEqual([]);
    expect(messages.size).toBeGreaterThan(500);
  });

  it('reads a message the same in single and double quotes', () => {
    expect(messageOf(`'It\\'s "{0}"\\n' + "done"`)).toBe('It\'s "{0}"\ndone');
    expect(messageOf(`"It's \\"{0}\\"\\n" + 'done'`)).toBe('It\'s "{0}"\ndone');
    expect(messageOf('`It is {0}`')).toBeUndefined();
  });

  for (const [name, catalogue] of Object.entries(catalogues)) {
    it(`${name} translates exactly the messages of the code`, () => {
      const missing = [...messages.keys()].filter((m) => !(m in catalogue));
      const extra = Object.keys(catalogue).filter((m) => !messages.has(m));
      expect(missing).toEqual([]);
      expect(extra).toEqual([]);
    });

    it(`${name} keeps the placeholders of every message`, () => {
      const broken = Object.entries(catalogue)
        .filter(([source, target]) => !target.trim() || placeholders(source) !== placeholders(target))
        .map(([source, target]) => `${source} => ${target}`);
      expect(broken).toEqual([]);
    });
  }
});

// ---------------------------------------------------------------------------------------------------------------
// Pseudo-locale rendering
// ---------------------------------------------------------------------------------------------------------------

vi.mock('../lib/api.js');

// Dates are written by Intl, in the locale of the interface or else of the browser, never by t: their words (an "AM",
// a month) are localized there, so the pseudo-locale counts them as translated.
vi.mock('../lib/format-date.js', async (importOriginal) => {
  const real = await importOriginal();
  return {
    ...real,
    formatDateTime: (...args) => `⟦${real.formatDateTime(...args)}⟧`,
    formatDateTimePrecise: (...args) => `⟦${real.formatDateTimePrecise(...args)}⟧`,
  };
});

// Words that are the same in every language: product name, protocol and data-format identifiers, and examples of
// code. Anything else visible must come from t.
const UNTRANSLATED = new Set([
  'Mimicway',
  'GET',
  'POST',
  'mock',
  'proxy',
  'no-rule',
  'Content-Type',
  'application/json',
  'orders.in',
  '/user/role',
  'Envelope/Body/id',
  'English',
  'Français',
]);

const marked = (text) => `⟦${text}⟧`;

/**
 * `text` with its marked (translated) parts replaced by `by`; markers nest when a translation is a value of another
 * one.
 */
function unmarked(text, by = ' ') {
  let rest = text;
  for (let previous = ''; previous !== rest;) {
    previous = rest;
    rest = rest.replace(/⟦[^⟦⟧]*⟧/g, by);
  }
  return rest.trim();
}

function untranslatedWords(container) {
  const found = [];
  const check = (text, where) => {
    const rest = unmarked(text);
    // Languages space a colon, a semicolon or a mark differently (French puts a space before it): written outside t,
    // one spacing would show in every language. Next to a blank only, so that a time (12:00) or a port (:9000) passes.
    if (/\s[:;!?]|[:;!?](\s|$)/.test(unmarked(text, 'x'))) found.push(`${where}: "${text.trim()}"`);
    if (!rest || UNTRANSLATED.has(rest)) return;
    for (const token of rest.split(/\s+/)) {
      if (/[A-Za-zÀ-ÿ]{2,}/.test(token) && !UNTRANSLATED.has(token)) found.push(`${where}: "${rest}"`);
    }
  };
  for (const { text, where } of visibleTexts(container)) check(text, where);
  return [...new Set(found)];
}

describe('pseudo-locale: no visible word escapes t', () => {
  beforeAll(() => {
    useTranslator((message, args) => marked(format(message, args)));
    answerApi(api);
  });

  afterEach(() => cleanup());

  afterAll(() => {
    useTranslator(null);
    setLocale('en');
  });

  for (const { name, screens } of SCREEN_GROUPS) {
    it(name, async () => {
      for (const screen of screens) {
        const { container } = render(await screen.component(), { props: screen.props ?? {} });
        await screen.open?.(container);
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(untranslatedWords(container), screen.id).toEqual([]);
        cleanup();
      }
    });
  }
});
