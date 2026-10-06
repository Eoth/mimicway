/**
 * Every sentence the UI shows goes through `t`.
 *
 * The English text is written once, where it is used, and it is also the key: a translation is one line in
 * `src/locales/<locale>.json`, and nothing else changes per language (no second copy of a message, of the markup or
 * of the logic). The language is the one the user picked, else the browser's when a catalogue exists for it, else
 * English.
 *
 * Rules that `src/tests/l10n.test.js` enforces: a message is a double-quoted string literal (or such literals
 * joined by `+`), never a template or a variable, so that it can be extracted; values go through `{0}`, `{1}`…
 * placeholders; each catalogue translates exactly the existing messages and keeps their placeholders.
 */
// A catalogue is a separate chunk, fetched only when its language is used: English costs nothing, and each added
// language weighs only on its own users.
const loaders = {
  fr: () => import('../locales/fr.json'),
};
const catalogues = { en: null };
const supported = (code) => code === 'en' || code in loaders;

/** The languages offered by the language switcher, each named in its own language. */
export const LOCALES = [
  { code: 'en', label: 'English' },
  { code: 'fr', label: 'Français' },
];

const STORAGE_KEY = 'mimicway-locale';

function detectLocale() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && supported(saved)) return saved;
  } catch {
    // Storage unavailable (private mode, sandboxed frame): fall back to the browser's languages.
  }
  const languages = typeof navigator === 'undefined' ? [] : (navigator.languages ?? [navigator.language]);
  for (const language of languages) {
    const code = String(language).toLowerCase().split('-')[0];
    if (supported(code)) return code;
  }
  return 'en';
}

const state = $state({ locale: 'en' });
let translator = null;

export function getLocale() {
  return state.locale;
}

async function apply(code) {
  if (!(code in catalogues)) catalogues[code] = (await loaders[code]()).default;
  state.locale = code;
  if (typeof document !== 'undefined') document.documentElement.lang = code;
}

/** Applies the language of the browser, or the one picked earlier; awaited before the interface is mounted. */
export async function initLocale() {
  await apply(detectLocale());
}

/** Applies `code` and remembers it as the user's choice. */
export async function setLocale(code) {
  if (!supported(code)) return;
  await apply(code);
  try {
    localStorage.setItem(STORAGE_KEY, code);
  } catch {
    // Not remembered across reloads, still applied now.
  }
}

/** `{0}`, `{1}`… replaced by the arguments; a placeholder with no argument is left as written. */
export function format(message, args) {
  return message.replace(/\{(\d+)\}/g, (placeholder, index) => {
    const value = args[Number(index)];
    return value === undefined || value === null ? placeholder : String(value);
  });
}

/** Replaces the catalogues with `translate(message, args)`, for tests (a pseudo-locale); `null` restores them. */
export function useTranslator(translate) {
  translator = translate;
}

export function t(message, ...args) {
  // Read first, so that every text calling `t` is re-rendered when the language changes.
  const locale = state.locale;
  if (translator) return translator(message, args);
  const catalogue = catalogues[locale];
  return format(catalogue?.[message] ?? message, args);
}

/**
 * A count with its noun. Each form is a whole message of its own, so that a language can word both as it needs;
 * the singular is used for exactly one. `n` is `{0}`, the other values follow as `{1}`, `{2}`…
 */
export function tCount(n, one, many, ...args) {
  return n === 1 ? t(one, n, ...args) : t(many, n, ...args);
}

/** Locale for dates and numbers: the UI's language when it has a regional form, else the browser's own. */
export function intlLocale() {
  return state.locale === 'fr' ? 'fr-FR' : undefined;
}
