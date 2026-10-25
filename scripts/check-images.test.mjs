// Tests of check-images.mjs: light and inert images that pass, each way of making a page heavy or an image active
// that fails, and the images of the repository. Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { MAX_IMAGE_BYTES, MAX_SVG_BYTES, checkImages, svgProblems } from './check-images.mjs';

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const svg = (body = '', style = '') =>
  `<svg xmlns="http://www.w3.org/2000/svg"><title>T</title><style>${style}</style>${body}</svg>`;
const MOVING = '.a{animation:1s infinite x}@keyframes x{to{opacity:0}}';

const check = (entries) =>
  checkImages(
    Object.keys(entries),
    (file) => entries[file].size ?? entries[file].text.length,
    (file) => entries[file].text,
  );

test('light images and an inert SVG that stops for reduced motion pass', () => {
  const result = check({
    'docs/en/screenshots/a.png': { size: MAX_IMAGE_BYTES },
    'docs/en/screenshots/demo.webp': { size: 120_000 },
    'docs/en/screenshots/how.svg': {
      text: svg('<a href="#t"><rect/></a>', `${MOVING}@media (prefers-reduced-motion:reduce){.a{animation:none}}`),
    },
    'frontend/public/favicon.svg': { text: '<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>' },
    'src/main.rs': { size: 10_000_000 },
  });
  assert.deepEqual(result, { problems: [], images: 4 });
});

test('an image over its weight fails, an SVG at a lower limit', () => {
  const { problems } = check({
    'docs/en/screenshots/big.webp': { size: MAX_IMAGE_BYTES + 1 },
    'docs/en/screenshots/big.svg': { text: svg(' '.repeat(MAX_SVG_BYTES)) },
  });
  assert.equal(problems.length, 2);
  assert.match(problems[0], /big\.webp: 150001 bytes/);
  assert.match(problems[1], /big\.svg: \d+ bytes, over the 10000/);
});

test('a video or a GIF fails, whatever its weight', () => {
  for (const file of ['docs/demo.mp4', 'docs/demo.webm', 'docs/en/screenshots/tour.gif', 'demo.MOV']) {
    const { problems } = check({ [file]: { size: 10 } });
    assert.equal(problems.length, 1, file);
    assert.match(problems[0], /no video or GIF/);
  }
});

test('each way of making an SVG active fails', () => {
  for (const [body, what] of [
    ['<script>alert(1)</script>', 'a script'],
    ['<rect onload="alert(1)"/>', 'an event handler'],
    ['<foreignObject><p>x</p></foreignObject>', 'a foreignObject'],
    ['<image href="https://example.com/x.png"/>', 'a link to another resource'],
    ['<use xlink:href="other.svg#a"/>', 'a link to another resource'],
    ['<rect style="fill:url(https://example.com/p)"/>', 'a resource loaded by url()'],
  ]) {
    assert.deepEqual(svgProblems('x.svg', svg(body)), [`x.svg: holds ${what}; an image must stay inert`], body);
  }
  assert.deepEqual(svgProblems('x.svg', svg('', '@import "https://example.com/a.css";')), [
    'x.svg: holds an @import; an image must stay inert',
  ]);
});

test('an animated SVG without a reduced-motion rule fails, and one without a title', () => {
  assert.deepEqual(svgProblems('x.svg', svg('', MOVING)), [
    'x.svg: is animated without a prefers-reduced-motion: reduce rule that stops it',
  ]);
  assert.deepEqual(svgProblems('x.svg', svg('<animate attributeName="x"/>')), [
    'x.svg: is animated without a prefers-reduced-motion: reduce rule that stops it',
  ]);
  assert.deepEqual(svgProblems('x.svg', '<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>'), [
    'x.svg: has no <title>, which a screen reader announces',
  ]);
});

test('the images of the repository pass, the demo and its diagram among them', () => {
  const files = execFileSync('git', ['ls-files'], { cwd: ROOT, encoding: 'utf8' }).split('\n').filter(Boolean);
  const { problems, images } = checkImages(
    files,
    (file) => statSync(path.join(ROOT, file)).size,
    (file) => readFileSync(path.join(ROOT, file), 'utf8'),
  );
  assert.deepEqual(problems, []);
  assert.ok(images > 0);
});
