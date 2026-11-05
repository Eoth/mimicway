// Tests of the ports the Rust tests hand to a server they do not bind themselves (a TCP mock, the real binary). Every
// Rust file of src/ and tests/, found by walking them, is read: a port read from a listener bound to port 0 must not
// outlive that listener, since the kernel hands a closed port of its ephemeral range to the next bind to port 0 or the
// next outgoing connection, the next call of the same helper included. Such ports come from src/test_ports.rs, which
// picks them outside that range. Run with: node --test scripts/*.test.mjs
//
// Not seen: a listener closed by the end of its scope while its port escapes (a block or a function that returns the
// port of a local listener), a listener dropped by a path other than drop(), and a port copied to another variable
// before the drop.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));

function rustFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return rustFiles(path);
    return entry.name.endsWith('.rs') ? [path] : [];
  });
}

// Returns one line per place where a port outlives the port-0 listener it was read from.
export function releasedPorts(source) {
  const problems = [];
  const lines = source.split('\n');
  const portZero = /bind\(\s*"(?:127\.0\.0\.1|0\.0\.0\.0|localhost|\[::1?\]):0"\s*\)/;
  // A listener that is never stored: its port is read from a temporary, dropped at the end of the statement.
  lines.forEach((line, i) => {
    if (portZero.test(line) && !/\blet\s+(?:mut\s+)?\w+\s*=/.test(line) && !/\(\s*listener|serve\(/.test(line)) {
      const statement = lines.slice(i, i + 4).join(' ');
      if (/local_addr\(\)/.test(statement)) problems.push(`${i + 1}: port read from a listener that is not kept`);
    }
  });
  // A stored listener dropped by hand, while the port read from it is used afterwards.
  lines.forEach((line, i) => {
    const bound = line.match(/\blet\s+(?:mut\s+)?(\w+)\s*=.*/);
    if (!bound || !portZero.test(lines.slice(i, i + 2).join(' '))) return;
    const name = bound[1];
    const dropped = lines.findIndex((later, j) => j > i && new RegExp(`\\bdrop\\(\\s*${name}\\s*\\)`).test(later));
    if (dropped === -1) return;
    const ports = lines
      .slice(i, dropped)
      .map((l) => l.match(new RegExp(`\\blet\\s+(\\w+)\\s*=\\s*${name}\\.local_addr\\(\\)`)))
      .filter(Boolean)
      .map((m) => m[1]);
    const after = lines.slice(dropped + 1).join('\n');
    for (const port of ports) {
      if (new RegExp(`\\b${port}\\b`).test(after)) {
        problems.push(`${dropped + 1}: ${name}, bound to port 0 on line ${i + 1}, is dropped before ${port} is used`);
      }
    }
  });
  return problems;
}

test('no Rust test hands over a port read from a port-0 listener it has closed', () => {
  const files = [...rustFiles(join(root, 'src')), ...rustFiles(join(root, 'tests'))];
  assert.ok(files.length > 50, `only ${files.length} Rust files found`);
  const problems = files.flatMap((file) =>
    releasedPorts(readFileSync(file, 'utf8')).map((p) => `${relative(root, file)}:${p}`),
  );
  assert.deepEqual(problems, []);
});

test('the shapes that release the port are found, the kept listeners are not', () => {
  const helper = `fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}`;
  assert.equal(releasedPorts(helper).length, 1);
  const probe = `let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
let port = probe.local_addr().unwrap().port();
drop(probe);
let created = create(port).await;`;
  assert.equal(releasedPorts(probe).length, 1);
  const split = `let probe = tokio::net::TcpListener::bind(
    "127.0.0.1:0").await.unwrap();
let port = probe.local_addr().unwrap().port();
drop( probe );
connect(("127.0.0.1", port));`;
  assert.equal(releasedPorts(split).length, 1);
  const kept = `let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
let port = listener.local_addr().unwrap().port();
tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });`;
  assert.deepEqual(releasedPorts(kept), []);
  const reserved = `let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
let taken_port = reserved.local_addr().unwrap().port();
assert!(!statuses[0].listening);
drop(reserved);`;
  // Held while the port is used, then dropped: the port never outlives it.
  assert.deepEqual(releasedPorts(reserved), []);
});
