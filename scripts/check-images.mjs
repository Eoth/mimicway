// Fails when an image of the repository would make a page heavy or could run something in a reader's browser:
// - every image Git tracks weighs at most MAX_IMAGE_BYTES, the demo of the README included: the guide is read on slow
//   connections too, and each byte is downloaded by every reader;
// - no video and no GIF: the demo is an animated WebP of a few screens, a video of the same screens weighs megabytes;
// - an SVG weighs at most MAX_SVG_BYTES and stays inert: no script, no event handler, no foreignObject, nothing loaded
//   from elsewhere (GitHub strips these in its own way; a copy served by anyone else would not);
// - an animated SVG stops moving for a reader who asks for less motion (prefers-reduced-motion), and an SVG of the
//   documentation has a <title>, which screen readers announce (the favicon is never announced).
// Every tracked file is listed, so a new image is checked without being named, and the summary counts what was read.
// Run with: node scripts/check-images.mjs; its tests: node --test scripts/*.test.mjs
import { execFileSync } from 'node:child_process';
import { readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const MAX_IMAGE_BYTES = 150_000;
export const MAX_SVG_BYTES = 10_000;
const IMAGE = /\.(png|jpe?g|webp|avif|svg)$/i;
const REFUSED = /\.(gif|mp4|webm|mov|avi|mkv|m4v|ogv)$/i;

/** The problems of one SVG, from its text; `shown` when a page displays it, so that it needs a title. */
export function svgProblems(file, text, shown = true) {
  const problems = [];
  const unsafe = [
    [/<script\b/i, 'a script'],
    [/\son[a-z]+\s*=/i, 'an event handler'],
    [/<foreignObject\b/i, 'a foreignObject'],
    [/\b(xlink:)?href\s*=\s*["'](?!#)/i, 'a link to another resource'],
    [/url\(\s*["']?(?!#)/i, 'a resource loaded by url()'],
    [/@import\b/i, 'an @import'],
  ];
  for (const [pattern, what] of unsafe) {
    if (pattern.test(text)) problems.push(`${file}: holds ${what}; an image must stay inert`);
  }
  if (/\banimation\s*:|@keyframes|<animate/i.test(text) && !/prefers-reduced-motion\s*:\s*reduce/i.test(text)) {
    problems.push(`${file}: is animated without a prefers-reduced-motion: reduce rule that stops it`);
  }
  if (shown && !/<title\b/i.test(text)) problems.push(`${file}: has no <title>, which a screen reader announces`);
  return problems;
}

/** The problems among `files` (repository-relative paths); `sizeOf(file)` gives bytes, `read(file)` the text. */
export function checkImages(files, sizeOf, read) {
  const problems = [];
  let images = 0;
  for (const file of files) {
    if (REFUSED.test(file)) {
      problems.push(`${file}: no video or GIF in the repository; show screens as an animated WebP`);
      continue;
    }
    if (!IMAGE.test(file)) continue;
    images += 1;
    const isSvg = /\.svg$/i.test(file);
    const limit = isSvg ? MAX_SVG_BYTES : MAX_IMAGE_BYTES;
    const size = sizeOf(file);
    if (size > limit) problems.push(`${file}: ${size} bytes, over the ${limit} bytes an image may weigh`);
    if (isSvg) problems.push(...svgProblems(file, read(file), file.startsWith('docs/')));
  }
  return { problems, images };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
  const files = execFileSync('git', ['ls-files'], { cwd: root, encoding: 'utf8' }).split('\n').filter(Boolean);
  const { problems, images } = checkImages(
    files,
    (file) => statSync(path.join(root, file)).size,
    (file) => readFileSync(path.join(root, file), 'utf8'),
  );
  for (const problem of problems) console.error(problem);
  if (problems.length > 0 || images === 0) process.exit(1);
  console.log(`${images} images, each within its weight, the SVGs inert; no video, no GIF.`);
}
