import catalog from './template-catalog.json';
import { t } from './i18n.svelte.js';
import { findPipeSeparator } from './tpl-utils.js';

// Help on response templates, for every builder: the pipes and fake data kinds they offer, the suggestions of a text
// field that holds `{{…}}` expressions, and the warnings on a pipe or an expression the engine would not read as
// meant. The names, examples and traps come from template-catalog.json, which the engine itself renders in a test
// (src/engine/template/catalog_judge.rs): a suggestion or a warning here says what the engine really does.

const PIPE_LABELS = {
  lower: () => t('lower — lowercase'),
  upper: () => t('upper — uppercase'),
  trim: () => t('trim — strip spaces'),
  capitalize: () => t('capitalize — first letter uppercase'),
  first: () => t('first(N) — first N characters'),
  last: () => t('last(N) — last N characters'),
  substr: () => t('substr(start,len)'),
  default: () => t('default("val") — when empty'),
  replace: () => t('replace("a","b")'),
  prepend: () => t('prepend("prefix")'),
  append: () => t('append("suffix")'),
  length: () => t('length — number of characters'),
  epoch_ms: () => t('epoch_ms — ISO date to milliseconds since 1970'),
  epoch_seconds: () => t('epoch_seconds — ISO date to seconds since 1970'),
  epoch_minutes: () => t('epoch_minutes — ISO date to minutes since 1970'),
  date: () => t('date("yyyy-MM-dd") — ISO date in another format'),
};

const VARIABLE_LABELS = {
  'path.': () => t('URL parameter {param}'),
  'query.': () => t('Query param'),
  'header.': () => t('HTTP header'),
  'body.': () => t('Echo of the body (JSON pointer)'),
  'xpath.': () => t('XPath (XML/SOAP)'),
  'fake.': () => t('Fake data'),
  uuid: () => t('UUID'),
  now_ms: () => t('Timestamp (ms)'),
  now_iso: () => t('ISO date'),
  seq: () => t('Sequence counter'),
  'script.': () => t('Script result'),
  '"': () => t('A text of your own, to transform with a pipe'),
};

// The pipes whose argument is a number: the engine reads anything else as 0 (first, last) or ignores it (substr).
const NUMBER_ARGUMENTS = { first: 1, last: 1, substr: 2 };

/** The pipe suggestions of every builder: `value` is what a pick inserts, a working example. */
export const pipeOptions = catalog.pipes.map((p) => ({
  value: p.insert,
  name: p.name,
  input: p.input,
  output: p.output,
  get label() {
    return PIPE_LABELS[p.name]();
  },
}));

/** The kinds of `{{fake.Kind}}`, in the order the builders list them. */
export const fakeKinds = catalog.fake;

const PIPE_NAMES = new Set(catalog.pipes.map((p) => p.name));
const NAMED_VARIABLES = catalog.variables.map((v) => v.insert).filter((v) => v !== '"');

function pipeNameOf(pipe) {
  const open = pipe.indexOf('(');
  return (open === -1 ? pipe : pipe.slice(0, open)).trim();
}

/** The pipes of a chain (`first(9) | upper`), split at the bars outside parentheses, as the engine splits them. */
export function splitPipes(chain) {
  const parts = [];
  let rest = chain;
  for (let at = findPipeSeparator(rest); at !== -1; at = findPipeSeparator(rest)) {
    parts.push(rest.slice(0, at));
    rest = rest.slice(at + 1);
  }
  parts.push(rest);
  return parts.map((p) => p.trim()).filter(Boolean);
}

/**
 * What the engine would do wrong with each pipe of a chain: `unknown` (left as it is: the value comes out unchanged),
 * `number` (an argument that should be a number is not, or is missing). An empty list when every pipe is read as
 * meant.
 */
export function pipeProblems(chain) {
  const problems = [];
  for (const pipe of splitPipes(chain ?? '')) {
    const name = pipeNameOf(pipe);
    if (!PIPE_NAMES.has(name)) {
      problems.push({ pipe, name, problem: 'unknown' });
      continue;
    }
    const count = NUMBER_ARGUMENTS[name];
    if (count) {
      const args = pipe.match(/^[a-z_]+\s*\((.*)\)$/);
      const numbers = args ? args[1].split(',').map((a) => a.trim()) : [];
      const ok = args && numbers.length >= (name === 'substr' ? 1 : count) && numbers.every((n) => /^\d+$/.test(n));
      if (!ok) problems.push({ pipe, name, problem: 'number' });
    }
  }
  return problems;
}

/** One sentence per problem of `pipeProblems`, for the line under a pipe field. */
export function describePipeProblems(chain) {
  return pipeProblems(chain).map(({ pipe, name, problem }) =>
    problem === 'unknown'
      ? t('“{0}” is not a known transformation: the value comes out unchanged.', pipe)
      : t('“{0}” needs whole numbers in parentheses, for example {1}.', pipe, exampleOf(name)),
  );
}

function exampleOf(name) {
  return catalog.pipes.find((p) => p.name === name)?.insert ?? name;
}

/** What a chain of known pipes does to the example of its first pipe, as `“abcdef” → “abc”`, or null. */
export function pipeExample(chain) {
  const pipes = splitPipes(chain ?? '');
  if (pipes.length !== 1 || pipeProblems(chain).length) return null;
  const option = pipeOptions.find((p) => p.value === pipes[0]);
  return option ? `“${option.input}” → “${option.output}”` : null;
}

/** The expressions `{{…}}` of a free text that the engine would render empty or not as meant. */
export function expressionProblems(text) {
  const problems = [];
  const value = text ?? '';
  let from = 0;
  for (let open = value.indexOf('{{', from); open !== -1; open = value.indexOf('{{', from)) {
    const close = closingBraces(value, open + 2);
    if (close === -1) {
      problems.push(t('“{{” opens an expression that no “}}” closes: the text is sent as it is.'));
      break;
    }
    const expr = value.slice(open + 2, close);
    const bar = barOutsideQuotes(expr);
    const variable = (bar === -1 ? expr : expr.slice(0, bar)).trim();
    if (!isKnownVariable(variable)) {
      problems.push(t('“{0}” is not a value Mimicway knows: it is replaced by an empty text.', `{{${expr}}}`));
    }
    if (bar !== -1) problems.push(...describePipeProblems(expr.slice(bar + 1)));
    from = close + 2;
  }
  return problems;
}

function isKnownVariable(variable) {
  if (/^"(?:[^"\\]|\\.)*"$/.test(variable)) return true;
  if (variable === 'script' || variable === 'now_epoch') return true;
  if (variable.startsWith('fake.')) return fakeKinds.includes(variable.slice(5));
  return NAMED_VARIABLES.some((v) =>
    v.endsWith('.') ? variable.startsWith(v) && variable.length > v.length : v === variable,
  );
}

// The engine's reading of `{{…}}` (find_closing_double_brace in template.rs): quotes and parentheses hide a `}}`.
function closingBraces(text, start) {
  let inQuotes = false;
  let depth = 0;
  for (let i = start; i < text.length; i++) {
    const c = text[i];
    if (c === '"' && depth === 0) inQuotes = !inQuotes;
    else if (c === '(' && !inQuotes) depth++;
    else if (c === ')' && !inQuotes) depth = Math.max(0, depth - 1);
    else if (c === '}' && !inQuotes && depth === 0 && text[i + 1] === '}') return i;
  }
  return -1;
}

// The engine's split of an expression (eval_expression in template.rs): the first bar outside double quotes.
function barOutsideQuotes(expr) {
  let inQuotes = false;
  for (let i = 0; i < expr.length; i++) {
    if (expr[i] === '"') inQuotes = !inQuotes;
    else if (expr[i] === '|' && !inQuotes) return i;
  }
  return -1;
}

/**
 * The suggestions for the caret of a free text: inside an expression `{{` opened before the caret and not closed, the
 * values Mimicway knows, then after a bar the pipes. `from`..`caret` is the part a pick replaces; `close` tells to add
 * the closing `}}` after a complete pick, when nothing closes the expression yet. Null outside an expression.
 */
export function suggestAt(text, caret) {
  const before = text.slice(0, caret);
  const open = before.lastIndexOf('{{');
  if (open === -1 || before.indexOf('}}', open) !== -1) return null;
  const inside = before.slice(open + 2);
  const close = closingBraces(text, open + 2) === -1;
  const bar = barOutsideQuotes(inside);
  if (bar !== -1) {
    const chain = inside.slice(bar + 1);
    const lastBar = chain.lastIndexOf('|');
    const segment = chain.slice(lastBar + 1);
    const typed = segment.trimStart();
    const from = caret - typed.length;
    const items = pipeOptions
      .filter((p) => p.value !== typed && (p.value.startsWith(typed) || p.name.startsWith(typed)))
      .map((p) => ({ insert: p.value, label: p.label, complete: true }));
    return { from, caret, close, items };
  }
  const typed = inside.trimStart();
  if (typed.startsWith('"')) return null;
  const from = caret - typed.length;
  const variables = typed.startsWith('fake.')
    ? fakeKinds.map((k) => ({ insert: `fake.${k}`, label: k, complete: true }))
    : catalog.variables.map((v) => ({
        insert: v.insert,
        label: VARIABLE_LABELS[v.insert](),
        complete: !v.insert.endsWith('.') && v.insert !== '"',
      }));
  const items = variables.filter((v) => v.insert.startsWith(typed) && v.insert !== typed);
  return { from, caret, close, items };
}

/** The text after a pick, and where the caret goes: after the pick, before any `}}` the pick added. */
export function applySuggestion(text, suggestion, item) {
  const closing = item.complete && suggestion.close ? '}}' : '';
  const next = text.slice(0, suggestion.from) + item.insert + closing + text.slice(suggestion.caret);
  return { text: next, caret: suggestion.from + item.insert.length };
}
