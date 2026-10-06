// The native Rhai functions Mimicway provides (the UI's counterpart of ScriptEngine::new in src/engine/script.rs). The
// script help ("Rhai examples and syntax", RuleResponseSection.svelte) and the autocompletion of the script editor
// (RhaiScriptEditor.svelte) both read this list, so a function added to the engine is described here once.
import { t } from './i18n.svelte.js';

// Texts are getters, read when shown, so that they follow a change of language.
export const RHAI_FUNCTIONS = [
  // The request: `request` is a variable, not a function, listed here for the same help and autocompletion.
  // ScriptEngine::execute() (src/engine/script.rs) puts one `request` object in the Rhai scope, with four fields:
  // body, headers, query, path. Reading a missing key is never an error in Rhai (neither `.key` nor `["key"]`), it
  // gives an empty value: a script that reads a misspelled key (`request.path.id` when the path parameter is
  // `orderId`) fails silently, whereas calling a function that does not exist is a real error. The rule tester
  // ("Test against a real request") shows whether a key is found.
  {
    name: 'request.path',
    get signature() {
      return t('request.path.parameter_name');
    },
    get description() {
      return t(
        'Path parameters taken from the URL (e.g. {id} in /orders/{id} -> request.path.id). A missing key gives an empty value, never an error.',
      );
    },
    insertText: 'request.path',
  },
  {
    name: 'request.query',
    get signature() {
      return t('request.query.parameter_name');
    },
    get description() {
      return t(
        'Parameters of the query string (e.g. ?page=2 -> request.query.page). A missing key gives an empty value, never an error.',
      );
    },
    insertText: 'request.query',
  },
  {
    name: 'request.headers',
    get signature() {
      return t('request.headers.header_name');
    },
    get description() {
      return t(
        'HTTP headers of the request. WARNING: the server always lowercases their names (e.g. "SOAPAction" becomes request.headers.soapaction): use a lowercase name, or the key is silently not found.',
      );
    },
    insertText: 'request.headers',
  },
  {
    name: 'request.body',
    signature: 'request.body',
    get description() {
      return t(
        'Raw body of the request, as text. Parse it with parse_json() or parse_xml_items() when it is structured.',
      );
    },
    insertText: 'request.body',
  },
  {
    name: 'random_int',
    signature: 'random_int(min, max)',
    get description() {
      return t('Random integer between min and max (inclusive).');
    },
    insertText: 'random_int(min, max)',
  },
  {
    name: 'now_ms',
    signature: 'now_ms()',
    get description() {
      return t('Current Unix timestamp in milliseconds.');
    },
    insertText: 'now_ms()',
  },
  {
    name: 'now_iso',
    signature: 'now_iso()',
    get description() {
      return t('Current date and time in full ISO 8601 format (with the time).');
    },
    insertText: 'now_iso()',
  },
  {
    name: 'year',
    signature: 'year()',
    get description() {
      return t('Current year, 4 digits.');
    },
    insertText: 'year()',
  },
  {
    name: 'date_now',
    signature: 'date_now(format)',
    get description() {
      return t('Today\'s date. Optional format: "iso" (default), "fr", "en".');
    },
    insertText: 'date_now("iso")',
  },
  {
    name: 'date_past',
    get signature() {
      return t('date_past(days, format)');
    },
    get description() {
      return t('A date in the past, "days" days before today (0 or negative = today).');
    },
    get insertText() {
      return t('date_past(days, "iso")');
    },
  },
  {
    name: 'date_future',
    get signature() {
      return t('date_future(days, format)');
    },
    get description() {
      return t('A date in the future, "days" days after today (0 or negative = today).');
    },
    get insertText() {
      return t('date_future(days, "iso")');
    },
  },
  {
    name: 'parse_date',
    get signature() {
      return t('parse_date(text, "pattern")');
    },
    get description() {
      return t(
        'The reverse of date_now/date_past/date_future: parses a date TYPED in an explicit pattern (yyyy/MM/dd/HH/mm/ss, any other character is literal) and returns milliseconds since the epoch. E.g. parse_date("15/03/2026", "dd/MM/yyyy"). The time is optional (00:00:00 by default). A run error when the text does not follow the pattern or the date does not exist (e.g. February 31).',
      );
    },
    get insertText() {
      return t('parse_date(text, "dd/MM/yyyy")');
    },
  },
  {
    name: 'uuid',
    signature: 'uuid()',
    get description() {
      return t('Random UUID v4 identifier.');
    },
    insertText: 'uuid()',
  },
  {
    name: 'fake',
    signature: 'fake("Kind")',
    get description() {
      return t('Fake data (e.g. "FirstName", "Email", "CompanyName"...).');
    },
    insertText: 'fake("FirstName")',
  },
  {
    name: 'seeded_int',
    signature: 'seeded_int(seed, min, max)',
    get description() {
      return t('Deterministic integer in [min, max]: the same seed always gives the same result.');
    },
    insertText: 'seeded_int(seed, min, max)',
  },
  {
    name: 'seeded_pick',
    get signature() {
      return t('seeded_pick(seed, [list])');
    },
    get description() {
      return t('Picks an element of the list, always the same one for the same seed.');
    },
    insertText: 'seeded_pick(seed, ["a", "b"])',
  },
  {
    name: 'parse_json',
    get signature() {
      return t('parse_json(text)');
    },
    get description() {
      return t('Parses JSON text (e.g. request.body) into a Rhai list or object you can navigate.');
    },
    insertText: 'parse_json(request.body)',
  },
  {
    name: 'to_json',
    get signature() {
      return t('to_json(value)');
    },
    get description() {
      return t('Serializes a Rhai list or object to JSON text, to insert through {{script.field}}.');
    },
    get insertText() {
      return t('to_json(value)');
    },
  },
  {
    name: 'parse_xml_items',
    get signature() {
      return t('parse_xml_items(text, "path/to/item")');
    },
    get description() {
      return t(
        'Extracts every XML element repeated at a path into a list of Rhai objects (one level of child fields).',
      );
    },
    get insertText() {
      return t('parse_xml_items(request.body, "path/to/item")');
    },
  },
  {
    name: 'xml_element',
    get signature() {
      return t('xml_element(tag, value)');
    },
    get description() {
      return t('Builds an XML element <tag>...</tag> from a Rhai list or object (recursive).');
    },
    get insertText() {
      return t('xml_element("tag", value)');
    },
  },
];

// The functions whose name starts with `query`, ignoring case; all of them when `query` is empty (Ctrl+Space before
// anything is typed).
export function filterRhaiFunctions(query) {
  if (!query) return RHAI_FUNCTIONS;
  const lower = query.toLowerCase();
  return RHAI_FUNCTIONS.filter((f) => f.name.toLowerCase().startsWith(lower));
}

// The Rhai identifier (letters, digits, underscores) right before `cursorPos` in `text`: both the prefix the
// autocompletion looks for and the text an inserted suggestion replaces.
export function tokenAtCursor(text, cursorPos) {
  const before = text.slice(0, cursorPos);
  const match = before.match(/[A-Za-z_][A-Za-z0-9_]*$/);
  return match ? { token: match[0], start: cursorPos - match[0].length } : { token: '', start: cursorPos };
}

// What to select right after inserting `insertText` (offsets within it): the arguments between the parentheses, so
// that typing replaces them ("random_int(min, max)" selects "min, max"); without arguments, the caret goes after the
// call ("now_ms()").
export function computeInsertSelection(insertText) {
  const openIdx = insertText.indexOf('(');
  const closeIdx = insertText.lastIndexOf(')');
  if (openIdx === -1 || closeIdx === -1 || closeIdx <= openIdx + 1) {
    return { start: insertText.length, end: insertText.length };
  }
  return { start: openIdx + 1, end: closeIdx };
}
