#!/usr/bin/env node
// Fails when the translations of the documentation drift apart. Each directory of docs/ is one language
// (docs/en/, docs/fr/…) holding the same file names, and README.md has one translation per other language
// (README.fr.md…). A reader who switches language must land on the same page, with the same sections and the same
// images; without this check, a page added, split or illustrated in one language silently leaves the others behind.
//
// For every page and every screenshot, it checks that each language has it; for every set of translations of a page
// (and of the README), that the first line links to each other language, that the headings have the same levels in
// the same order, and that the same images appear in the same order.
// Usage: node scripts/check-doc-translations.mjs [repository root] (defaults to this script's repository).
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// The language of README.md, and the one listed first when versions disagree.
const SOURCE_LANGUAGE = 'en';

function withoutCode(text) {
  return text.replace(/```[\s\S]*?```/g, '').replace(/`[^`\n]*`/g, '');
}

function headingLevels(text) {
  return (withoutCode(text).match(/^#{1,6} /gm) ?? []).map((marks) => marks.trim().length);
}

function levelCounts(levels) {
  const counts = [0, 0, 0, 0, 0, 0];
  for (const level of levels) counts[level - 1] += 1;
  return counts
    .map((count, index) => (count ? `h${index + 1}×${count}` : ''))
    .filter(Boolean)
    .join(' ');
}

// Image targets, with the page's own language written as {lang}: README.fr.md shows docs/fr/screenshots/x.png where
// README.md shows docs/en/screenshots/x.png, and both are the same image.
function images(text, language) {
  const own = new RegExp(`(^|/)${language}/`);
  return [...withoutCode(text).matchAll(/!\[[^\]]*\]\(([^)\s]+)/g)].map(([, target]) =>
    target.replace(own, '$1{lang}/'),
  );
}

function firstLine(text) {
  return text.replace(/^﻿/, '').split(/\r?\n/, 1)[0];
}

// `versions`: the translations of one page, as { language, file, text }, source language first.
function compareVersions(versions, problems) {
  for (const version of versions) {
    const top = firstLine(version.text);
    for (const other of versions) {
      if (other === version) continue;
      const link = path.posix.relative(path.posix.dirname(version.file), other.file);
      if (!top.includes(`](${link})`)) {
        problems.push(`${version.file}: the first line does not link to the ${other.language} version (${link})`);
      }
    }
  }
  const [reference, ...others] = versions;
  const referenceLevels = headingLevels(reference.text);
  const referenceImages = images(reference.text, reference.language);
  for (const version of others) {
    const levels = headingLevels(version.text);
    if (levels.join() !== referenceLevels.join()) {
      const counts = levelCounts(levels);
      const referenceCounts = levelCounts(referenceLevels);
      const how = counts === referenceCounts ? 'in another order' : `${counts} against ${referenceCounts}`;
      problems.push(`${version.file}: headings differ from ${reference.file} (${how})`);
    }
    const shown = images(version.text, version.language);
    if (shown.join('\n') !== referenceImages.join('\n')) {
      const missing = referenceImages.filter((image) => !shown.includes(image));
      const extra = shown.filter((image) => !referenceImages.includes(image));
      const detail =
        missing.length || extra.length
          ? [missing.length && `missing ${missing.join(', ')}`, extra.length && `extra ${extra.join(', ')}`]
              .filter(Boolean)
              .join('; ')
          : 'same images in another order';
      problems.push(`${version.file}: images differ from ${reference.file} (${detail})`);
    }
  }
}

function sourceFirst(languages) {
  return [...languages].sort((a, b) => (a === SOURCE_LANGUAGE ? -1 : b === SOURCE_LANGUAGE ? 1 : a.localeCompare(b)));
}

/**
 * Returns the problems found among `files` (repository-relative paths with `/` separators); `read(file)` returns the
 * text of a Markdown file.
 */
export function checkTranslations(files, read) {
  const problems = [];
  const trees = new Map();
  for (const file of files) {
    if (!file.startsWith('docs/')) continue;
    const match = /^docs\/([^/]+)\/(.+)$/.exec(file);
    if (!match) {
      problems.push(`${file}: outside a language directory (docs/<language>/…)`);
      continue;
    }
    const [, language, name] = match;
    if (!trees.has(language)) trees.set(language, new Set());
    trees.get(language).add(name);
  }
  const languages = sourceFirst(trees.keys());

  const names = [...new Set([...trees.values()].flatMap((names) => [...names]))].sort();
  for (const name of names) {
    const present = languages.filter((language) => trees.get(language).has(name));
    for (const language of languages) {
      if (!present.includes(language)) {
        problems.push(`docs/${language}/${name}: missing (present in ${present.map((l) => `docs/${l}/`).join(', ')})`);
      }
    }
    if (name.endsWith('.md') && present.length > 1) {
      const versions = present.map((language) => {
        const file = `docs/${language}/${name}`;
        return { language, file, text: read(file) };
      });
      compareVersions(versions, problems);
    }
  }

  const readmes = new Map();
  for (const file of files) {
    const match = /^README(?:\.([a-z]{2}(?:-[A-Z]{2})?))?\.md$/.exec(file);
    if (match) readmes.set(match[1] ?? SOURCE_LANGUAGE, file);
  }
  for (const language of languages) {
    if (!readmes.has(language)) {
      problems.push(
        `${language === SOURCE_LANGUAGE ? 'README.md' : `README.${language}.md`}: missing (the guide exists in docs/${language}/)`,
      );
    }
  }
  const readmeVersions = sourceFirst(readmes.keys()).map((language) => {
    const file = readmes.get(language);
    return { language, file, text: read(file) };
  });
  if (readmeVersions.length > 1) compareVersions(readmeVersions, problems);

  return problems;
}

function main() {
  const root = path.resolve(process.argv[2] ?? path.join(path.dirname(fileURLToPath(import.meta.url)), '..'));
  // The files Git tracks, so that a local run checks exactly what CI checks.
  const files = execFileSync('git', ['ls-files', 'docs', 'README*.md'], { cwd: root, encoding: 'utf8' })
    .split('\n')
    .filter(Boolean);
  const problems = checkTranslations(files, (file) => readFileSync(path.join(root, file), 'utf8'));
  if (problems.length) {
    console.error(problems.join('\n'));
    console.error(`\n${problems.length} translation problem(s).`);
    process.exit(1);
  }
  console.log('Every page and screenshot exists in every language, with the same headings and images.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
