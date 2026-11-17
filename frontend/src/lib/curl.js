// The command that sends a call kept by traffic observation again (ObservedCalls.svelte), built here only: curl, read by
// a POSIX shell (sh, bash, zsh). A value that holds anything but plain characters goes between single quotes, the one
// quoting where the shell reads nothing ($, backticks, backslashes, newlines and any other character stay as they
// are); a single quote ends the quoting, is written escaped (\'), and the quoting starts again.
//
// No value taken from the traffic reaches an option of curl that reads its value as an instruction: the body is written
// with --data-raw, where `@` names no file (--data-binary and --data would read `@/proc/self/environ` and send that
// file), a header goes out only under a name that is an HTTP token, so that `--header` never sees `@file` either, and
// the address is read with --globoff, so that braces and brackets stay characters.
//
// The command carries what observation kept, nothing more: a credential kept as [redacted] stays so, a body cut at the
// size limit is sent cut, a body that was not UTF-8 is sent as the text kept. The screen says each of these. A body
// that holds a character a command must not hold (`UNSAFE`, NUL among them) is read from the file the screen downloads
// (`bodyFileName`).

const PLAIN = /^[A-Za-z0-9_@%+=:,./-]+$/;

/** `value` as one word of a POSIX shell, quoted only when it needs to be. */
export function shellArg(value) {
  const text = String(value);
  if (PLAIN.test(text)) return text;
  return `'${text.replaceAll("'", `'\\''`)}'`;
}

// A header name as HTTP defines it (RFC 9110, token).
const TOKEN = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;

// What a copied command never holds: the control characters but tab and line feed (C0, DEL, C1), which a terminal acts
// on when they are pasted (ESC [ 2 0 1 ~ ends a protected paste, and what follows runs without Enter), and the
// invisible characters that reorder or hide text (bidirectional controls, marks, separators of lines and paragraphs,
// the byte order mark), which make a command read otherwise than it runs.
const UNSAFE = /[\u0000-\u0008\u000B-\u001F\u007F-\u009F\u061C\u200E\u200F\u2028-\u202E\u2066-\u2069\uFEFF]/;

// Headers curl writes itself from the URL and the body: the kept ones would be wrong for the command (a body cut, a
// host where the call came through).
const WRITTEN_BY_CURL = new Set(['host', 'content-length', 'transfer-encoding', 'connection', 'expect']);

/** Whether a text holds a character a copied command must never hold (`UNSAFE`). */
export function holdsUnsafe(text) {
  return UNSAFE.test(String(text));
}

/** Whether the request body must be read from its file: it holds a character a command must not (NUL among them). */
export function bodyNeedsFile(call) {
  return holdsUnsafe(call.request.body.text);
}

/**
 * Why no command is offered for a call, or null: a value other than the body (which goes through its file) holds a
 * character a command must not hold. The server keeps no such value today; a command is refused rather than built.
 */
export function commandRefusal(call, origin) {
  const { method, request } = call;
  const values = [origin, method, request.path, request.query ?? '', ...Object.entries(request.headers).flat()];
  return values.some(holdsUnsafe) ? 'unsafe' : null;
}

/** The name the request body downloads under, its extension taken from its media type. */
export function bodyFileName(call) {
  const type = (call.request.body.content_type ?? '').toLowerCase();
  const extension = type.includes('json') ? 'json' : type.includes('xml') ? 'xml' : 'txt';
  return `request-body-${call.id}.${extension}`;
}

/** The arguments of the command after `curl`, as the shell must hand them over. */
export function curlArgs(call, origin) {
  const { method, request } = call;
  const body = request.body.text;
  const args = ['--globoff'];
  if (method === 'HEAD') args.push('--head');
  else if (method !== 'GET' || body !== '') args.push('--request', method);
  for (const [name, value] of Object.entries(request.headers)) {
    if (!TOKEN.test(name) || WRITTEN_BY_CURL.has(name.toLowerCase())) continue;
    // `name:` alone would remove the header: curl sends an empty value for `name;`.
    args.push('--header', value === '' ? `${name};` : `${name}: ${value}`);
  }
  // The file is the one the screen downloads, its name written here, never taken from the call.
  if (body !== '')
    args.push(...(bodyNeedsFile(call) ? ['--data-binary', `@${bodyFileName(call)}`] : ['--data-raw', body]));
  // curl drops a `?` that nothing follows: an empty query is sent as none.
  const query = request.query ? `?${request.query}` : '';
  args.push(`${origin}${request.path}${query}`);
  return args;
}

/** The whole command, one option per line; null when none is offered (`commandRefusal`). */
export function curlCommand(call, origin) {
  if (commandRefusal(call, origin)) return null;
  const words = ['curl'];
  const args = curlArgs(call, origin);
  const lines = [];
  for (let i = 0; i < args.length; i++) {
    const takesValue = ['--request', '--header', '--data-binary', '--data-raw'].includes(args[i]);
    lines.push(takesValue ? `${args[i]} ${shellArg(args[++i])}` : shellArg(args[i]));
  }
  return [words.concat(lines.shift()).join(' '), ...lines].join(' \\\n  ');
}
