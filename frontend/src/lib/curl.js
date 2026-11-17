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
// size limit is sent cut, a body that was not UTF-8 is sent as the text kept. The screen says each of these. A command
// line cannot carry a NUL character: such a body is read from the file the screen downloads (`bodyFileName`).

const PLAIN = /^[A-Za-z0-9_@%+=:,./-]+$/;

/** `value` as one word of a POSIX shell, quoted only when it needs to be. */
export function shellArg(value) {
  const text = String(value);
  if (PLAIN.test(text)) return text;
  return `'${text.replaceAll("'", `'\\''`)}'`;
}

// Headers curl writes itself from the URL and the body: the kept ones would be wrong for the command (a body cut, a
// host where the call came through).
// A header name as HTTP defines it (RFC 9110, token).
const TOKEN = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;

const WRITTEN_BY_CURL = new Set(['host', 'content-length', 'transfer-encoding', 'connection', 'expect']);

/** Whether the request body must be read from its file: a command line cannot carry a NUL character. */
export function bodyNeedsFile(call) {
  return call.request.body.text.includes('\0');
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

/** The whole command, one option per line. */
export function curlCommand(call, origin) {
  const words = ['curl'];
  const args = curlArgs(call, origin);
  const lines = [];
  for (let i = 0; i < args.length; i++) {
    const takesValue = ['--request', '--header', '--data-binary', '--data-raw'].includes(args[i]);
    lines.push(takesValue ? `${args[i]} ${shellArg(args[++i])}` : shellArg(args[i]));
  }
  return [words.concat(lines.shift()).join(' '), ...lines].join(' \\\n  ');
}
