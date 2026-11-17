// The command copied from a kept call is judged by a real shell: sh (and bash when the machine has it) runs it with a
// stand-in curl that prints the arguments it received, which must be exactly those the call gives, whatever the
// characters of its path, query, headers and body (quotes, newlines, $, backticks, backslashes, non-ASCII).
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import fc from 'fast-check';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { curlArgs, curlCommand, shellArg, bodyFileName, bodyNeedsFile } from '../lib/curl.js';

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
      '--data-binary',
      '<ping/>',
      'http://localhost:7342/orders/ws?a=1&a=2',
    ]);
  });

  it('names no method curl would choose itself, and keeps an empty query', () => {
    const get = { ...kept({ headers: {}, body: { text: '', content_type: null }, query: '' }), method: 'GET' };
    expect(curlArgs(get, 'http://h')).toEqual(['--globoff', 'http://h/orders/ws?']);
    const head = { ...get, method: 'HEAD', request: { ...get.request, query: null } };
    expect(curlArgs(head, 'http://h')).toEqual(['--globoff', '--head', 'http://h/orders/ws']);
    const getWithBody = { ...kept({ headers: {} }), method: 'GET' };
    expect(curlArgs(getWithBody, 'http://h').slice(0, 3)).toEqual(['--globoff', '--request', 'GET']);
  });

  it('reads a body holding a NUL character from the file it downloads under', () => {
    const binary = kept({ body: { text: 'a\0b', content_type: 'application/json' } });
    expect(bodyNeedsFile(binary)).toBe(true);
    expect(bodyFileName(binary)).toBe('request-body-7.json');
    expect(curlArgs(binary, 'http://h')).toContain('@request-body-7.json');
    expect(bodyFileName(kept())).toBe('request-body-7.xml');
    expect(bodyFileName(kept({ body: { text: 'x', content_type: null } }))).toBe('request-body-7.txt');
  });

  it('writes one option per line', () => {
    expect(curlCommand(kept({ headers: {} }), 'http://h')).toBe(
      "curl --globoff \\\n  --request POST \\\n  --data-binary '<ping/>' \\\n  'http://h/orders/ws?a=1&a=2'",
    );
  });
});
