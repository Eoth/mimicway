// The command copied from a kept call is judged by a real shell: sh (and bash when the machine has it) runs it with a
// stand-in curl that prints the arguments it received, which must be exactly those the call gives, whatever the
// characters of its path, query, headers and body (quotes, newlines, $, backticks, backslashes, non-ASCII).
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import fc from 'fast-check';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { curlArgs, curlCommand, shellArg, bodyFileName, bodyNeedsFile, commandRefusal } from '../lib/curl.js';

let bin;
// By their full path: the command runs with the stand-in curl alone on PATH.
const shells = ['sh', 'bash']
  .map((shell) => spawnSync('sh', ['-c', `command -v ${shell}`], { encoding: 'utf8' }).stdout.trim())
  .filter((found) => found.startsWith('/'));

beforeAll(() => {
  bin = fs.mkdtempSync(path.join(os.tmpdir(), 'curl-judge-'));
  fs.writeFileSync(path.join(bin, 'curl'), '#!/bin/sh\nprintf \'%s\\0\' "$@"\n', { mode: 0o755 });
});

afterAll(() => {
  fs.rmSync(bin, { recursive: true, force: true });
});

/** The arguments `shell` hands to curl when it runs `command`. */
function argumentsReceived(shell, command) {
  const run = spawnSync(shell, ['-c', command], { env: { PATH: bin }, encoding: 'utf8' });
  expect(run.status, run.stderr).toBe(0);
  return run.stdout.split('\0').slice(0, -1);
}

// Characters a shell reads, mixed with ordinary and non-ASCII ones; NUL never: no command line carries it.
const awkward = fc.string({
  unit: fc.constantFrom(
    "'",
    '"',
    '$',
    '`',
    '\\',
    '\n',
    '\r',
    '\t',
    ' ',
    '!',
    '*',
    '?',
    '~',
    '#',
    ';',
    '&',
    '|',
    '(',
    ')',
    '<',
    '>',
    '{',
    '}',
    '[',
    ']',
    '=',
    '%',
    '-',
    'a',
    'Z',
    '0',
    'é',
    '中',
    '😀',
  ),
});
const text = fc.oneof(
  awkward,
  fc.string({ unit: 'grapheme' }).filter((s) => !s.includes('\0')),
);
const token = fc.stringMatching(/^[A-Za-z][A-Za-z0-9-]{0,15}$/);

const call = fc.record({
  id: fc.nat(),
  method: fc.constantFrom('GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'HEAD', 'OPTIONS'),
  request: fc.record({
    path: text.map((p) => `/${p}`),
    query: fc.option(text, { nil: null }),
    headers: fc.dictionary(token, text, { maxKeys: 5 }),
    body: fc.record({ text, content_type: fc.option(token, { nil: null }) }),
  }),
});

// Each case starts shells: under the load of the whole suite, more than the default five seconds.
const SHELL_TIMEOUT_MS = 60000;

describe('the command of a kept call', () => {
  it(
    'hands curl exactly the arguments of the call, in every shell',
    () => {
      fc.assert(
        fc.property(call, fc.constantFrom('http://localhost:7342', "http://it's.test"), (generated, origin) => {
          const command = curlCommand(generated, origin);
          fc.pre(command !== null);
          for (const shell of shells) {
            expect(argumentsReceived(shell, command)).toEqual(curlArgs(generated, origin));
          }
        }),
        { numRuns: 150 },
      );
    },
    SHELL_TIMEOUT_MS,
  );

  it(
    'quotes a word only when the shell would read something in it',
    () => {
      fc.assert(
        fc.property(text, (value) => {
          expect(argumentsReceived(shells[0], `curl ${shellArg(value)}`)).toEqual([value]);
        }),
        { numRuns: 100 },
      );
      expect(shellArg('application/json')).toBe('application/json');
      expect(shellArg('')).toBe("''");
      expect(shellArg("it's $HOME")).toBe(`'it'\\''s $HOME'`);
    },
    SHELL_TIMEOUT_MS,
  );
});

describe('what the command says', () => {
  const kept = (overrides = {}) => ({
    id: 7,
    method: 'POST',
    request: {
      path: '/orders/ws',
      query: 'a=1&a=2',
      headers: {
        authorization: '[redacted]',
        'content-length': '12',
        'content-type': 'text/xml',
        host: 'mimic.test',
        'x-empty': '',
      },
      body: { text: '<ping/>', content_type: 'text/xml' },
      ...overrides,
    },
  });

  it('sends the method, the headers curl does not write itself, the body and the address', () => {
    expect(curlArgs(kept(), 'http://localhost:7342')).toEqual([
      '--globoff',
      '--request',
      'POST',
      '--header',
      'authorization: [redacted]',
      '--header',
      'content-type: text/xml',
      '--header',
      'x-empty;',
      '--data-raw',
      '<ping/>',
      'http://localhost:7342/orders/ws?a=1&a=2',
    ]);
  });

  it('names no method curl would choose itself, and sends an empty query as none, as curl would', () => {
    const get = { ...kept({ headers: {}, body: { text: '', content_type: null }, query: '' }), method: 'GET' };
    expect(curlArgs(get, 'http://h')).toEqual(['--globoff', 'http://h/orders/ws']);
    const head = { ...get, method: 'HEAD', request: { ...get.request, query: null } };
    expect(curlArgs(head, 'http://h')).toEqual(['--globoff', '--head', 'http://h/orders/ws']);
    const getWithBody = { ...kept({ headers: {} }), method: 'GET' };
    expect(curlArgs(getWithBody, 'http://h').slice(0, 3)).toEqual(['--globoff', '--request', 'GET']);
  });

  // Every character a pasted command must not hold, one of each class, at the start, inside and at the end of a value.
  const UNSAFE_SAMPLES = [
    ...Array.from({ length: 32 }, (_, c) => String.fromCharCode(c)).filter((c) => c !== '\t' && c !== '\n'),
    '\u007f',
    ...Array.from({ length: 32 }, (_, c) => String.fromCharCode(0x80 + c)),
    '\u061c',
    '\u200e',
    '\u200f',
    '\u2028',
    '\u2029',
    '\u202a',
    '\u202b',
    '\u202c',
    '\u202d',
    '\u202e',
    '\u2066',
    '\u2067',
    '\u2068',
    '\u2069',
    '\ufeff',
  ];

  it('sends a body holding a control or invisible character from its file, never in the command', () => {
    for (const c of UNSAFE_SAMPLES) {
      for (const text of [`${c}x`, `x${c}y`, `x${c}`, `x\u001b[201~\u0015touch PWNED\r`]) {
        const unsafe = kept({ body: { text, content_type: null } });
        const command = curlCommand(unsafe, 'http://h');
        expect(command, JSON.stringify(text)).toContain('--data-binary @request-body-7.txt');
        expect(command.includes(c), JSON.stringify(c)).toBe(false);
      }
    }
    // Tab and line feed stay in the command, as typed.
    expect(curlCommand(kept({ body: { text: 'a\tb\nc', content_type: null } }), 'http://h')).toContain(
      "--data-raw 'a\tb\nc'",
    );
  });

  it('offers no command for a call whose other values hold such a character', () => {
    for (const c of UNSAFE_SAMPLES) {
      for (const overrides of [{ headers: { 'x-a': `v${c}` } }, { path: `/a${c}` }, { query: `q=${c}` }]) {
        const call = kept(overrides);
        expect(commandRefusal(call, 'http://h'), JSON.stringify(overrides)).toBe('unsafe');
        expect(curlCommand(call, 'http://h')).toBeNull();
      }
    }
    expect(curlCommand(kept(), 'http://h\u001b')).toBeNull();
    const cut = kept();
    cut.request.head_truncated = true;
    expect(commandRefusal(cut, 'http://h')).toBe('cut');
    expect(curlCommand(cut, 'http://h')).toBeNull();
    expect(commandRefusal(kept(), 'http://h')).toBeNull();
  });

  it('never writes a control or invisible character in a command, whatever the call', () => {
    const anyChar = fc.oneof(fc.constantFrom(...UNSAFE_SAMPLES), fc.string({ unit: 'grapheme', maxLength: 3 }));
    const anyText = fc.array(anyChar, { maxLength: 6 }).map((p) => p.join(''));
    const anyCall = fc.record({
      id: fc.nat(),
      method: fc.constantFrom('GET', 'POST', 'HEAD'),
      request: fc.record({
        path: anyText.map((p) => `/${p}`),
        query: fc.option(anyText, { nil: null }),
        headers: fc.dictionary(anyText, anyText, { maxKeys: 3 }),
        body: fc.record({ text: anyText, content_type: fc.constant(null) }),
      }),
    });
    fc.assert(
      fc.property(anyCall, (generated) => {
        const command = curlCommand(generated, 'http://h');
        if (command === null) return;
        for (const c of UNSAFE_SAMPLES) expect(command.includes(c), JSON.stringify(c)).toBe(false);
      }),
      { numRuns: 500 },
    );
  });

  it('reads a body holding a NUL character from the file it downloads under', () => {
    const binary = kept({ body: { text: 'a\0b', content_type: 'application/json' } });
    expect(bodyNeedsFile(binary)).toBe(true);
    expect(bodyFileName(binary)).toBe('request-body-7.json');
    expect(curlArgs(binary, 'http://h')).toContain('@request-body-7.json');
    expect(bodyFileName(kept())).toBe('request-body-7.xml');
    expect(bodyFileName(kept({ body: { text: 'x', content_type: null } }))).toBe('request-body-7.txt');
  });

  it('never hands curl a body or a header it could read as a file', () => {
    const args = curlArgs(
      kept({
        headers: { '@/etc/passwd': 'x', 'x-ok': '@/etc/hostname' },
        body: { text: '@/proc/self/environ', content_type: null },
      }),
      'http://h',
    );
    expect(args).toContain('--data-raw');
    expect(args).not.toContain('--data-binary');
    expect(args.filter((a, i) => args[i - 1] === '--header')).toEqual(['x-ok: @/etc/hostname']);
  });

  it('writes one option per line', () => {
    expect(curlCommand(kept({ headers: {} }), 'http://h')).toBe(
      "curl --globoff \\\n  --request POST \\\n  --data-raw '<ping/>' \\\n  'http://h/orders/ws?a=1&a=2'",
    );
  });
});
