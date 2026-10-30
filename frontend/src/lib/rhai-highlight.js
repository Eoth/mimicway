// Splits a Rhai script into the pieces the script editor colors (RhaiScriptEditor.svelte). Only the look depends on
// it: the engine never reads this, and a piece colored wrongly changes nothing to what the script does.
//
// The whole rule, in the order it is tried at each position:
//   comment   `//` to the end of the line; `/*` to its matching `*/` (Rhai nests them), or to the end of the text
//   string    `"…"` and `'…'` with `\` escapes, to the closing quote or the end of the line; `` `…` `` to the closing
//             backtick, across lines, its `${…}` included
//   number    a digit, then digits, letters, `_` and `.` followed by a digit (`42`, `1_000`, `0xff`, `1.5e3`)
//   keyword   a word of RHAI_KEYWORDS (the editor's completion), or `catch`, `until`, `this`, which only come with one
//   function  any other word followed by `(`, spaces allowed between: a native function, a method after `.`, a call
//   plain     everything else, one character at a time, merged with the plain pieces around it
// The pieces, joined, give back the text exactly (rhai-highlight.test.js checks it on generated texts): the colored
// copy lies under the text area, character for character.
import { RHAI_KEYWORDS } from './rhai-functions.js';

const KEYWORDS = new Set([...RHAI_KEYWORDS.map((keyword) => keyword.name), 'catch', 'until', 'this']);

const WORD = /[A-Za-z_][A-Za-z0-9_]*/y;
const NUMBER = /[0-9](?:[0-9A-Za-z_]|\.(?=[0-9]))*/y;
const CALL_AFTER = /\s*\(/y;

function matchAt(pattern, text, at) {
  pattern.lastIndex = at;
  const match = pattern.exec(text);
  return match ? match[0].length : 0;
}

// The end of the block comment opened at `at`, nested ones included.
function blockCommentEnd(text, at) {
  let depth = 0;
  let i = at;
  while (i < text.length) {
    if (text.startsWith('/*', i)) {
      depth += 1;
      i += 2;
    } else if (text.startsWith('*/', i)) {
      depth -= 1;
      i += 2;
      if (depth === 0) return i;
    } else {
      i += 1;
    }
  }
  return text.length;
}

// The end of the string opened at `at` by `quote`; only a backtick string goes past the end of a line.
function stringEnd(text, at, quote) {
  let i = at + 1;
  while (i < text.length) {
    const c = text[i];
    if (c === '\\') i += 2;
    else if (c === quote) return i + 1;
    else if (c === '\n' && quote !== '`') return i;
    else i += 1;
  }
  return text.length;
}

function lineEnd(text, at) {
  const newline = text.indexOf('\n', at);
  return newline === -1 ? text.length : newline;
}

// The piece that starts at `at`: [kind, end], kind null for plain text.
function pieceAt(text, at) {
  const c = text[at];
  if (text.startsWith('//', at)) return ['comment', lineEnd(text, at)];
  if (text.startsWith('/*', at)) return ['comment', blockCommentEnd(text, at)];
  if (c === '"' || c === "'" || c === '`') return ['string', stringEnd(text, at, c)];
  const number = matchAt(NUMBER, text, at);
  if (number) return ['number', at + number];
  const word = matchAt(WORD, text, at);
  if (word) {
    const end = at + word;
    if (KEYWORDS.has(text.slice(at, end))) return ['keyword', end];
    return [matchAt(CALL_AFTER, text, end) ? 'function' : null, end];
  }
  return [null, at + 1];
}

/** The pieces of `text`, as `{ text, kind }`; kind is null for plain text. */
export function highlightRhai(text) {
  const pieces = [];
  let at = 0;
  while (at < text.length) {
    const [kind, end] = pieceAt(text, at);
    const last = pieces[pieces.length - 1];
    if (kind === null && last && last.kind === null) last.text += text.slice(at, end);
    else pieces.push({ text: text.slice(at, end), kind });
    at = end;
  }
  return pieces;
}
