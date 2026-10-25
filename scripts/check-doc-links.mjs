#!/usr/bin/env node
// Fails when a Markdown file of the repository links to a file, an image or a heading anchor that does not exist.
// Pages and screenshots get renamed; without this check, a broken link or a missing image only shows up when a
// reader clicks it. Usage: node scripts/check-doc-links.mjs (from anywhere in the repository).
import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
// The files Git tracks, so that a local run checks exactly what CI checks.
function markdownFiles() {
  return execFileSync('git', ['ls-files', '*.md'], { cwd: root, encoding: 'utf8' })
    .split('\n')
    .filter(Boolean)
    .map((name) => path.join(root, name));
}

// GitHub's anchor for a heading: lowercase, punctuation dropped, spaces turned into dashes.
function slug(heading) {
  return heading
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\- _]/gu, '')
    .replace(/ /g, '-');
}

function withoutCode(text) {
  return text.replace(/```[\s\S]*?```/g, '').replace(/`[^`\n]*`/g, '');
}

function anchorsOf(file) {
  const headings = withoutCode(readFileSync(file, 'utf8')).match(/^#{1,6} .+$/gm) ?? [];
  return new Set(headings.map((h) => slug(h.replace(/^#+ /, ''))));
}

const anchorCache = new Map();
const problems = [];
for (const file of markdownFiles()) {
  const text = withoutCode(readFileSync(file, 'utf8'));
  // Markdown links and images, then the images of HTML blocks (<img src>, <source srcset>).
  const targets = [
    ...[...text.matchAll(/\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)].map(([, target]) => target),
    ...[...text.matchAll(/<(?:img|source)\b[^>]*?\s(?:src|srcset)="([^"\s]+)"/g)].map(([, target]) => target),
  ];
  for (const target of targets) {
    if (/^(https?:|mailto:)/.test(target)) continue;
    const [pathPart, fragment] = target.split('#');
    const resolved = pathPart ? path.resolve(path.dirname(file), decodeURIComponent(pathPart)) : file;
    const where = `${path.relative(root, file)}: ${target}`;
    if (!existsSync(resolved)) {
      problems.push(`missing file   ${where}`);
      continue;
    }
    if (fragment && resolved.endsWith('.md')) {
      if (!anchorCache.has(resolved)) anchorCache.set(resolved, anchorsOf(resolved));
      if (!anchorCache.get(resolved).has(fragment)) problems.push(`missing anchor ${where}`);
    }
  }
}

if (problems.length) {
  console.error(problems.join('\n'));
  console.error(`\n${problems.length} broken link(s).`);
  process.exit(1);
}
console.log('Every Markdown link, image and anchor resolves.');
