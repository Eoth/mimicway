// Tests of release-archive.sh: the archive of a release depends on the binary, the documents and the commit time
// alone, not on where, when or with which permissions the files were checked out, and a checkout with Windows line
// endings is refused. The script runs on Linux, as in the release workflow; elsewhere these tests are skipped.
// Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'release-archive.sh');
const DOCS = ['LICENSE', 'README.md', 'CHANGELOG.md', 'MIGRATING.md'];
const EPOCH = '1700000000';
const LINUX = process.platform === 'linux' ? false : 'the script runs on Linux, like the release workflow';
const hasZip = spawnSync('zip', ['-v']).status === 0;

// A checkout holding the documents and a binary, written in the given order, with the given times and permissions.
function checkout({ order = DOCS, time = 1_600_000_000, mode = 0o644, text = (doc) => `${doc}\n` } = {}) {
  const dir = mkdtempSync(path.join(tmpdir(), 'release-archive-'));
  for (const doc of order) {
    const file = path.join(dir, doc);
    writeFileSync(file, text(doc));
    chmodSync(file, mode);
    utimesSync(file, time, time);
  }
  writeFileSync(path.join(dir, 'mimicway'), 'binary\n');
  chmodSync(path.join(dir, 'mimicway'), mode);
  return dir;
}

// Packs in the given time zone: a zip stores local time, which the script must pin.
function pack(dir, target, zone = 'UTC') {
  return spawnSync('bash', [SCRIPT, 'mimicway', target, '1.2.3', 'out'], {
    cwd: dir,
    encoding: 'utf8',
    env: { ...process.env, SOURCE_DATE_EPOCH: EPOCH, TZ: zone },
  });
}

function archiveOf(dir, target, zone) {
  const result = pack(dir, target, zone);
  assert.equal(result.status, 0, result.stderr);
  const extension = target.includes('windows') ? 'zip' : 'tar.gz';
  return readFileSync(path.join(dir, 'out', `mimicway-1.2.3-${target}.${extension}`));
}

test('two checkouts that differ in place, order, times and permissions give the same .tar.gz', { skip: LINUX }, () => {
  const first = checkout();
  const second = checkout({ order: [...DOCS].reverse(), time: 1_650_000_000, mode: 0o600 });
  try {
    const target = 'x86_64-unknown-linux-musl';
    assert.ok(archiveOf(first, target).equals(archiveOf(second, target, 'Pacific/Kiritimati')));
  } finally {
    rmSync(first, { recursive: true, force: true });
    rmSync(second, { recursive: true, force: true });
  }
});

test(
  'the .tar.gz holds one directory, sorted, owned by root, at the commit time, with fixed permissions',
  { skip: LINUX },
  () => {
    const dir = checkout({ mode: 0o600 });
    try {
      archiveOf(dir, 'aarch64-unknown-linux-musl');
      const listing = spawnSync(
        'tar',
        ['--numeric-owner', '--full-time', '-tvzf', 'out/mimicway-1.2.3-aarch64-unknown-linux-musl.tar.gz'],
        { cwd: dir, encoding: 'utf8', env: { ...process.env, TZ: 'UTC' } },
      );
      assert.equal(listing.status, 0, listing.stderr);
      const entries = listing.stdout
        .trim()
        .split('\n')
        .map((line) => line.split(/\s+/))
        .map(([mode, owner, , date, time, name]) => `${mode} ${owner} ${date} ${time} ${name}`);
      const root = 'mimicway-1.2.3-aarch64-unknown-linux-musl';
      const at = '2023-11-14 22:13:20';
      assert.deepEqual(entries, [
        `drwxr-xr-x 0/0 ${at} ${root}/`,
        `-rw-r--r-- 0/0 ${at} ${root}/CHANGELOG.md`,
        `-rw-r--r-- 0/0 ${at} ${root}/LICENSE`,
        `-rw-r--r-- 0/0 ${at} ${root}/MIGRATING.md`,
        `-rw-r--r-- 0/0 ${at} ${root}/README.md`,
        `-rwxr-xr-x 0/0 ${at} ${root}/mimicway`,
      ]);
      // No time and no file name in the gzip header: bytes 4 to 7 hold the time, flag 8 announces a name.
      const gzip = readFileSync(path.join(dir, 'out', `${root}.tar.gz`));
      assert.equal(gzip.readUInt32LE(4), 0);
      assert.equal(gzip[3] & 0x08, 0);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  },
);

test(
  'two checkouts that differ in times, permissions and time zone give the same zip for Windows, without extra fields',
  { skip: LINUX || (!hasZip && 'zip is not installed') },
  () => {
    const first = checkout();
    const second = checkout({ time: 1_650_000_000, mode: 0o600 });
    try {
      const target = 'x86_64-pc-windows-msvc';
      const archive = archiveOf(first, target);
      assert.ok(archive.equals(archiveOf(second, target, 'Pacific/Kiritimati')));
      // The central directory: no extra field (owner, Unix times), and the commit time in UTC as a DOS time and date.
      const end = archive.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
      const entries = [];
      for (let i = 0, at = archive.readUInt32LE(end + 16); i < archive.readUInt16LE(end + 10); i++) {
        const [nameLength, extraLength, commentLength] = [28, 30, 32].map((offset) =>
          archive.readUInt16LE(at + offset),
        );
        entries.push({ time: archive.readUInt16LE(at + 12), date: archive.readUInt16LE(at + 14), extraLength });
        at += 46 + nameLength + extraLength + commentLength;
      }
      // 2023-11-14 22:13:20: (22 << 11) | (13 << 5) | (20 / 2), and ((2023 - 1980) << 9) | (11 << 5) | 14.
      assert.deepEqual(entries, Array(6).fill({ time: 45482, date: 22382, extraLength: 0 }));
      const listing = spawnSync('unzip', ['-Z1', `out/mimicway-1.2.3-${target}.zip`], { cwd: first, encoding: 'utf8' });
      assert.deepEqual(listing.stdout.trim().split('\n'), [
        `mimicway-1.2.3-${target}/`,
        `mimicway-1.2.3-${target}/CHANGELOG.md`,
        `mimicway-1.2.3-${target}/LICENSE`,
        `mimicway-1.2.3-${target}/MIGRATING.md`,
        `mimicway-1.2.3-${target}/README.md`,
        `mimicway-1.2.3-${target}/mimicway.exe`,
      ]);
    } finally {
      rmSync(first, { recursive: true, force: true });
      rmSync(second, { recursive: true, force: true });
    }
  },
);

test('a document with Windows line endings is refused, with the way out', { skip: LINUX }, () => {
  const dir = checkout({ text: (doc) => (doc === 'README.md' ? 'line\r\n' : `${doc}\n`) });
  try {
    const result = pack(dir, 'x86_64-unknown-linux-musl');
    assert.equal(result.status, 1);
    assert.match(
      result.stderr,
      /README\.md has Windows line endings: check the repository out with core\.autocrlf=false/,
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
