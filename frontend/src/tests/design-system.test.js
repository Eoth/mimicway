// Holds the interface to Phasme, its design system (src/tokens.css, docs/design-system.md). Every rule is mechanical:
// a color that bypasses the theme, a variable read and never defined, or a pair of colors under its contrast threshold
// looks right in one theme and wrong in the other, and no screenshot taken in one theme shows it. Each rule is first
// shown failing on a sample, then run on the sources.
import { describe, expect, test } from 'vitest';
import {
  TOKENS_FILE,
  block,
  contrast,
  definedProperties,
  inlineStyles,
  literalColors,
  primitives,
  readProperties,
  resolve,
  selectorClasses,
  styleSources,
  unscaledValues,
} from './design-rules.js';

const { tokens, sources } = styleSources();

/** Problems of every source, as "file:line: ...", for a check on style text. */
const everywhere = (check) =>
  sources.flatMap(({ file, css, markup }) => [
    ...check(css).map((problem) => `${file}:${problem}`),
    ...check(inlineStyles(markup)).map((problem) => `${file} (inline style) ${problem}`),
  ]);

describe('a literal color outside the tokens file', () => {
  test('is caught: hexadecimal, function, name, and in a fallback', () => {
    const sample =
      'a { color: #fff; }\nb { background: rgba(0, 0, 0, 0.2); border-color: white; }\n' +
      'c { color: var(--color-text, #202020); white-space: nowrap; background: transparent; }';
    expect(literalColors(sample)).toEqual([
      '1: color: #fff',
      '2: background: rgba(0, 0, 0, 0.2)',
      '2: border-color: white',
      '3: color: var(--color-text, #202020)',
    ]);
    const markup = '<p style="color: red">a</p><p style:background-color="#fff">b</p>';
    expect(literalColors(inlineStyles(markup))).toEqual(['1: color: red', '2: background-color: #fff']);
  });

  test(`appears in no component and no style sheet but ${TOKENS_FILE}`, () => {
    expect(everywhere(literalColors)).toEqual([]);
  });
});

describe('a variable read and defined nowhere', () => {
  const defined = new Set(
    [tokens, ...sources.flatMap((s) => [s.css, s.markup])].flatMap((t) => [...definedProperties(t)]),
  );

  test('is caught', () => {
    const missing = [...readProperties('a { color: var(--color-error-text, red); }')].filter((n) => !defined.has(n));
    expect(missing).toEqual(['--color-error-text']);
  });

  test('is read by no component and no style sheet', () => {
    const missing = sources.flatMap(({ file, css, markup }) =>
      [...readProperties(css + markup)].filter((name) => !defined.has(name)).map((name) => `${file}: ${name}`),
    );
    expect(missing).toEqual([]);
  });
});

describe('a primitive read outside the tokens file', () => {
  const raw = primitives(tokens);

  test('is caught', () => {
    expect([...readProperties('a { color: var(--moss-600); }')].filter((n) => raw.has(n))).toEqual(['--moss-600']);
  });

  test('is read by no component and no style sheet: they read roles', () => {
    expect(raw.size).toBeGreaterThan(20);
    const readers = sources.flatMap(({ file, css, markup }) =>
      [...readProperties(css + markup)].filter((name) => raw.has(name)).map((name) => `${file}: ${name}`),
    );
    expect(readers).toEqual([]);
  });
});

describe('a font, a size of text, a spacing, a radius, a layer or a shadow that is not a token', () => {
  test('is caught', () => {
    const sample =
      'a { font-family: monospace; border-radius: 50%; z-index: 30; box-shadow: 0 1px 2px var(--x); }\n' +
      'b { font-family: var(--font-code); border-radius: var(--radius-m); z-index: var(--z-modal); }\n' +
      'c { font-family: inherit; box-shadow: none; box-shadow: 0 0 0 var(--line-thick) var(--color-focus); }\n' +
      'd { font-size: 0.8125rem; padding: 0.5rem var(--space-2); margin: 0 auto; gap: 6px; line-height: 1.2; }\n' +
      'e { font-size: var(--text-s); padding: 0 var(--space-2); margin: calc(-1 * var(--space-1)) auto; line-height: var(--leading-body); }';
    expect(unscaledValues(sample)).toEqual([
      '1: font-family: monospace',
      '1: border-radius: 50%',
      '1: z-index: 30',
      '1: box-shadow: 0 1px 2px var(--x)',
      '4: font-size: 0.8125rem',
      '4: padding: 0.5rem var(--space-2)',
      '4: gap: 6px',
      '4: line-height: 1.2',
    ]);
  });

  test('appears in no component and no style sheet', () => {
    expect(everywhere(unscaledValues)).toEqual([]);
  });
});

describe('a class of app.css styled again by a component', () => {
  const shared = selectorClasses(sources.find(({ file }) => file === 'app.css').css);

  test('is caught', () => {
    const component = '.card { padding: 0; }\n.card .btn-icon { width: 1rem; }\n.btn-xs:hover { color: inherit; }';
    expect([...selectorClasses(component)].filter((name) => shared.has(name))).toEqual(['btn-icon', 'btn-xs']);
  });

  test('appears in no component: a class two components need lives in app.css, once', () => {
    expect(shared.size).toBeGreaterThan(40);
    const restyled = sources
      .filter(({ file }) => file.endsWith('.svelte'))
      .flatMap(({ file, css }) =>
        [...selectorClasses(css)].filter((name) => shared.has(name)).map((name) => `${file}: .${name}`),
      );
    expect(restyled).toEqual([]);
  });
});

// Pairs that carry information, per theme: text on the backgrounds it is set on (WCAG 2.2 1.4.3, 4.5:1), and the
// boundaries of controls, the focus ring and the mode lines on theirs (1.4.11, 3:1).
const TEXT = 4.5;
const GRAPHIC = 3;
const PAIRS = [
  ...[
    '--color-bg',
    '--color-surface',
    '--color-sunken',
    '--color-hover',
    '--color-selected',
    '--color-mock-bg',
    '--color-proxy-bg',
  ].flatMap((bg) => [
    ['--color-text', bg, TEXT],
    ['--color-text-muted', bg, TEXT],
  ]),
  ...['--color-bg', '--color-surface', '--color-sunken', '--color-selected'].map((bg) => ['--color-primary', bg, TEXT]),
  ['--color-on-primary', '--color-primary', TEXT],
  ['--color-on-primary', '--color-primary-hover', TEXT],
  ['--color-on-success', '--color-success', TEXT],
  ['--color-on-danger', '--color-danger', TEXT],
  ['--color-on-danger', '--color-danger-hover', TEXT],
  ...['mock', 'proxy', 'success', 'warning', 'danger', 'info'].flatMap((role) => [
    [`--color-${role}-text`, `--color-${role}-bg`, TEXT],
    [`--color-${role}`, '--color-surface', TEXT],
    [`--color-${role}`, '--color-bg', TEXT],
    [`--color-${role}`, `--color-${role}-bg`, GRAPHIC],
  ]),
  ...['--color-bg', '--color-surface', '--color-sunken'].flatMap((bg) => [
    ['--color-control', bg, GRAPHIC],
    ['--color-focus', bg, GRAPHIC],
  ]),
  ['--color-focus', '--color-selected', GRAPHIC],
];

describe('contrasts', () => {
  const base = block(tokens, ':root');
  const themes = {
    light: block(tokens, ':root, [data-theme="light"]'),
    dark: block(tokens, '[data-theme="dark"]'),
  };

  test('a pair under its threshold is caught', () => {
    const theme = new Map([
      ['--a', '#777777'],
      ['--b', '#ffffff'],
    ]);
    expect(contrast(resolve('--a', theme, base), resolve('--b', theme, base))).toBeLessThan(TEXT);
  });

  test('both themes define the same roles', () => {
    expect([...themes.dark.keys()].sort()).toEqual([...themes.light.keys()].sort());
  });

  for (const [name, theme] of Object.entries(themes)) {
    test(`every declared pair meets WCAG AA in the ${name} theme`, () => {
      const failures = PAIRS.map(([fg, bg, min]) => {
        const ratio = contrast(resolve(fg, theme, base), resolve(bg, theme, base));
        return ratio >= min ? null : `${fg} on ${bg}: ${ratio.toFixed(2)} < ${min}`;
      }).filter(Boolean);
      expect(failures).toEqual([]);
    });
  }
});
