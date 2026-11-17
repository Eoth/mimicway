// The command copied from a kept call, judged by the real curl: sh runs each generated command, curl sends it to a
// listener of this test, and the request received must be the call kept, byte for byte (method, path and query,
// every header the command writes, body), whatever the characters of its values: `@` and `<` (which curl reads as
// "take a file" in some options), `-`, `;`, braces, brackets, quotes, spaces, non-ASCII. A witness file sits in the
// directory the command runs in, named by those values: its content must never arrive.
//
// What it does not see: shells other than sh (the quoting is one POSIX rule, judged for bash too by the stand-in
// curl of curl.properties.test.js), curl versions other than the machine's, headers curl adds on its own (User-Agent,
// Accept, a Content-Type for a body without one), which the command leaves to curl, and HTTP/2. One difference is
// known and allowed: an empty query (`/x?`) leaves as none (`/x`), curl drops the `?`.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import fc from 'fast-check';
import { spawnSync, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import net from 'node:net';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { curlArgs, curlCommand, bodyFileName, bodyNeedsFile } from '../lib/curl.js';

const run = promisify(execFile);
const CURL = spawnSync('sh', ['-c', 'command -v curl'], { encoding: 'utf8' }).stdout.trim();
const WITNESS = `witness-content-${process.pid}`;
let dir;
let server;
let origin;
const received = [];

/** Whether `bytes` holds a whole request: its headers, then the body its Content-Length announces. */
function complete(bytes) {
  const end = bytes.indexOf('\r\n\r\n');
  if (end < 0) return false;
  const length = /\r\ncontent-length: *(\d+)/i.exec(bytes.subarray(0, end).toString('latin1'));
  return bytes.length >= end + 4 + (length ? Number(length[1]) : 0);
}

beforeAll(async () => {
  dir = fs.mkdtempSync(path.join(os.tmpdir(), 'curl-real-'));
  for (const name of ['witness.txt', 'witness', '-witness']) fs.writeFileSync(path.join(dir, name), WITNESS);
  server = net.createServer((socket) => {
    let bytes = Buffer.alloc(0);
    socket.on('data', (chunk) => {
      bytes = Buffer.concat([bytes, chunk]);
      if (complete(bytes)) {
        received.push(bytes);
        socket.end('HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n');
      }
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  origin = `http://127.0.0.1:${server.address().port}`;
});

afterAll(async () => {
  await new Promise((resolve) => server.close(resolve));
  fs.rmSync(dir, { recursive: true, force: true });
});

/** The request as curl sent it: the request line, the header lines, the body. */
function parse(bytes) {
  const end = bytes.indexOf('\r\n\r\n');
  const [line, ...headers] = bytes.subarray(0, end).toString('utf8').split('\r\n');
  return { line, headers, body: bytes.subarray(end + 4) };
}

// Values as observation keeps them: a header name is a token, a value visible ASCII with inner spaces and tabs, a path
// and a query characters an HTTP request line carries (percent escapes whole), a body any text.
const tricky = ['@witness.txt', '<witness.txt', '@witness', '-witness', ';type=text/plain', '@-', '{a}', '[1-2]'];
const visible = fc.constantFrom(...'@<>;-{}[]"\'\\$`!#%&*+,./:=?^_|~aZ09');
const headerValue = fc
  .array(fc.oneof(visible, fc.constantFrom(' ', '\t'), fc.constantFrom(...tricky)), { maxLength: 12 })
  .map((parts) => parts.join('').trim());
const pathChar = fc.oneof(
  fc.constantFrom(..."@-;=!$&'()*+,:aZ09_~"),
  fc.constantFrom('%41', '%2F', '%E9'),
  fc.constantFrom('@witness.txt', '-witness', ';type=x'),
);
const segment = fc
  .array(pathChar, { minLength: 1, maxLength: 6 })
  .map((p) => p.join(''))
  .filter((s) => s !== '.' && s !== '..');
// Control characters among them: such a body leaves through its file, which the test writes as the download would.
const control = fc.constantFrom('\0', '\u001b[201~', '\r', '\u0015', '\u007f', '\u0085', '\u202e', '\ufeff');
const body = fc.oneof(
  fc.constantFrom(...tricky),
  fc
    .array(fc.oneof(fc.constantFrom(...tricky), control, fc.string({ unit: 'grapheme' })), { maxLength: 6 })
    .map((p) => p.join('')),
  fc
    .array(fc.oneof(fc.constantFrom(...tricky), fc.string({ unit: 'grapheme' })), { maxLength: 6 })
    .map((p) => p.join('')),
);

const call = fc.record({
  id: fc.nat(),
  method: fc.constantFrom('GET', 'POST', 'PUT', 'DELETE', 'PATCH'),
  request: fc.record({
    path: fc.array(segment, { minLength: 1, maxLength: 3 }).map((s) => `/${s.join('/')}`),
    query: fc.option(
      fc.array(fc.oneof(pathChar, fc.constantFrom('?', '/')), { maxLength: 8 }).map((p) => p.join('')),
      { nil: null },
    ),
    headers: fc.dictionary(fc.stringMatching(/^[A-Za-z][A-Za-z0-9-]{0,10}$/), headerValue, { maxKeys: 4 }),
    body: fc.record({
      text: body,
      content_type: fc.constant('text/plain'),
    }),
  }),
});

describe('the command of a kept call, sent by the real curl', () => {
  it('finds curl on this machine', () => {
    expect(CURL, 'curl is needed by this judge: install it').toMatch(/^\//);
  });

  it('sends exactly the request kept, and never a file named by the call', async () => {
    await fc.assert(
      fc.asyncProperty(call, async (kept) => {
        if (bodyNeedsFile(kept)) fs.writeFileSync(path.join(dir, bodyFileName(kept)), kept.request.body.text);
        received.length = 0;
        await run('sh', ['-c', `${curlCommand(kept, origin)} --silent --show-error`], { cwd: dir });
        expect(received).toHaveLength(1);
        const request = parse(received[0]);
        expect(received[0].includes(WITNESS)).toBe(false);
        const query = kept.request.query ? `?${kept.request.query}` : '';
        const method = curlArgs(kept, origin).includes('--request') ? kept.method : request.line.split(' ')[0];
        expect(request.line).toBe(`${method} ${kept.request.path}${query} HTTP/1.1`);
        for (const [name, value] of Object.entries(kept.request.headers)) {
          expect(request.headers).toContain(value === '' ? `${name}:` : `${name}: ${value}`);
        }
        expect(request.body.toString('utf8')).toBe(kept.request.body.text);
      }),
      { numRuns: 120 },
    );
  }, 120000);
});
