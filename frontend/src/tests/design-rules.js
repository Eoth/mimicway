// Reads the style sources of the interface for design-system.test.js: the tokens file, the global style sheet, the
// <style> block of every component and the inline styles of its markup, and checks them against the rules of Phasme.
// Each check takes plain text, so that the tests can show it failing on a sample before running it on the sources.
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const SRC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const TOKENS_FILE = 'tokens.css';

const read = (file) => readFileSync(path.join(SRC, file), 'utf8');
export const stripComments = (css) => css.replace(/\/\*[\s\S]*?\*\//g, (comment) => comment.replace(/[^\n]/g, ' '));

/** The tokens file, and every other style source as { file, css, markup }, comments blanked out. */
export function styleSources() {
  const sources = readdirSync(SRC)
    .filter((file) => file.endsWith('.css') && file !== TOKENS_FILE)
    .map((file) => ({ file, css: stripComments(read(file)), markup: '' }));
  const inDir = (dir) => readdirSync(path.join(SRC, dir)).map((f) => `${dir}/${f}`);
  const components = ['App.svelte', ...inDir('lib/components'), ...inDir('preview')];
  // Both views keep the file's lines, the other part blanked out, so that a problem is reported at its real line.
  const blank = (text) => text.replace(/[^\n]/g, ' ');
  for (const file of components.filter((f) => f.endsWith('.svelte'))) {
    const text = read(file);
    const style = /<style[^>]*>([\s\S]*?)<\/style>/g;
    const css = text
      .split(style)
      .map((part, i) => (i % 2 ? part : blank(part)))
      .join('');
    const markup = text.replace(style, blank).replace(/<!--[\s\S]*?-->/g, blank);
    sources.push({ file, css: stripComments(css), markup });
  }
  return { tokens: stripComments(read(TOKENS_FILE)), sources };
}

/** Declarations `property: value` of a style text, with their line. */
export function declarations(css) {
  const found = [];
  for (const match of css.matchAll(/([a-z-]+|--[\w-]+)\s*:\s*([^;{}]+)(?=[;}]|$)/g)) {
    found.push({ property: match[1], value: match[2].trim(), line: css.slice(0, match.index).split('\n').length });
  }
  return found;
}

/** The style text of the inline styles of a markup: style="..." attributes and style:property="..." directives. */
export function inlineStyles(markup) {
  const parts = [];
  for (const m of markup.matchAll(/\sstyle="([^"]*)"/g)) parts.push(m[1]);
  for (const m of markup.matchAll(/\sstyle:([\w-]+)="([^"]*)"/g)) parts.push(`${m[1]}: ${m[2]}`);
  return parts.join(';\n');
}

// The named colors of CSS Color 4; `transparent` and `currentcolor` are not a color of the palette and stay allowed.
const NAMED =
  'aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown ' +
  'burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan ' +
  'darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred ' +
  'darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue ' +
  'dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray ' +
  'green greenyellow grey honeydew hotpink indianred indigo ivory khaki lavender lavenderblush lawngreen ' +
  'lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray lightgreen lightgrey lightpink ' +
  'lightsalmon lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen ' +
  'linen magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue ' +
  'mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy ' +
  'oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip ' +
  'peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown ' +
  'seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen steelblue tan teal ' +
  'thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen';
const LITERAL_COLOR = new RegExp(
  String.raw`#[0-9a-f]{3,8}\b|\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color|color-mix)\(|(?<![\w-])(?:${NAMED.split(' ').join('|')})(?![\w-])`,
  'i',
);

/** Literal colors of a style text, fallbacks of var() included, as "line: property: value". */
export function literalColors(css) {
  return declarations(css)
    .filter(({ value }) => LITERAL_COLOR.test(value.replace(/var\(\s*--[\w-]+/g, 'var(')))
    .map(({ line, property, value }) => `${line}: ${property}: ${value}`);
}

export const definedProperties = (text) => new Set(Array.from(text.matchAll(/(--[\w-]+)\s*:/g), (m) => m[1]));
export const readProperties = (text) => new Set(Array.from(text.matchAll(/var\(\s*(--[\w-]+)/g), (m) => m[1]));

/** Classes named in the selectors of a style text. */
export function selectorClasses(css) {
  const classes = new Set();
  for (const [, selector] of css.matchAll(/([^{}]+)\{[^{}]*\}/g)) {
    for (const [, name] of selector.matchAll(/\.(-?[_a-zA-Z][\w-]*)/g)) classes.add(name);
  }
  return classes;
}

/** Primitives: the tokens whose value is a literal (a color, or a color with transparency). */
export function primitives(tokens) {
  return new Set(
    declarations(tokens)
      .filter(({ property, value }) => property.startsWith('--') && /^#[0-9a-f]{3,8}$/i.test(value))
      .map(({ property }) => property),
  );
}

// Properties whose value comes from a scale, and what a value may hold besides the scale's tokens. A spacing may be 0,
// auto, or a negative step written calc(-1 * var(--space-N)).
const SPACING = /^(0|auto|calc\(\s*-1\s*\*\s*\)|\s)*$/;
const SCALED = {
  'font-family': /^(inherit)?$/,
  'border-radius': /^$/,
  'z-index': /^$/,
  'box-shadow': /^(none|inset|0|[\s,])*$/,
  'font-size': /^(inherit)?$/,
  'line-height': /^$/,
  ...Object.fromEntries(
    ['padding', 'margin']
      .flatMap((p) => [p, `${p}-top`, `${p}-right`, `${p}-bottom`, `${p}-left`])
      .concat(['gap', 'row-gap', 'column-gap'])
      .map((property) => [property, SPACING]),
  ),
};

/** Values of scaled properties that do not come from a token, as "line: property: value". */
export function unscaledValues(css) {
  return declarations(css)
    .filter(
      ({ property, value }) => property in SCALED && !SCALED[property].test(value.replace(/var\([^)]*\)/g, ' ').trim()),
    )
    .map(({ line, property, value }) => `${line}: ${property}: ${value}`);
}

/**
 * The custom properties of the first block whose selector list is exactly `selector`, values as written. Spaces and
 * the kind of quotes do not matter: the formatter decides them.
 */
export function block(tokens, selector) {
  const escaped = selector
    .replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    .replace(/\s+/g, '\\s*')
    .replace(/["']/g, `["']`);
  const match = new RegExp(`(?:^|})\\s*${escaped}\\s*\\{([^}]*)\\}`).exec(tokens);
  if (!match) throw new Error(`no block ${selector} in ${TOKENS_FILE}`);
  return new Map(
    declarations(match[1])
      .filter((d) => d.property.startsWith('--'))
      .map((d) => [d.property, d.value]),
  );
}

/** The color a token resolves to in a theme, following var() through the theme block and then the primitives. */
export function resolve(name, theme, base) {
  let value = theme.get(name) ?? base.get(name);
  for (let hops = 0; value && hops < 8; hops++) {
    const ref = /^var\((--[\w-]+)\)$/.exec(value);
    if (!ref) break;
    value = theme.get(ref[1]) ?? base.get(ref[1]);
  }
  if (!value || !/^#[0-9a-f]{6}$/i.test(value))
    throw new Error(`${name} does not resolve to an opaque color (${value})`);
  return value;
}

const channel = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const luminance = (hex) => {
  const [r, g, b] = [1, 3, 5].map((i) => channel(parseInt(hex.slice(i, i + 2), 16) / 255));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
/** WCAG 2.2 contrast ratio of two opaque colors. */
export function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
