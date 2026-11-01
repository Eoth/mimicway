// Tests of frontend/e2e/disposable-server.mjs, the check the end-to-end suite runs before its first test: a fake server
// records every request it gets, so that a refusal is shown to come before any request that writes. Run with:
// node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertDisposableServer, dataDir } from '../frontend/e2e/disposable-server.mjs';

const repository = fileURLToPath(new URL('../', import.meta.url));
const okDataPath = { DATA_PATH: path.join(os.tmpdir(), 'mimicway-e2e') };

async function fakeServer(health) {
  const requests = [];
  const server = http.createServer((req, res) => {
    requests.push(`${req.method} ${req.url}`);
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify(health));
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  const url = `http://127.0.0.1:${server.address().port}`;
  return { url, requests, close: () => new Promise((done) => server.close(done)) };
}

test('a server started without DISPOSABLE_DATA is refused after one read of its health', async () => {
  for (const health of [
    { status: 'ok' },
    { status: 'ok', disposable_data: false },
    { status: 'ok', disposable_data: 'true' },
  ]) {
    const server = await fakeServer(health);
    await assert.rejects(assertDisposableServer(server.url, { env: okDataPath }), /DISPOSABLE_DATA=true/);
    assert.deepEqual(server.requests, ['GET /api/health']);
    await server.close();
  }
});

test('a server that reports disposable data is accepted', async () => {
  const server = await fakeServer({ status: 'ok', disposable_data: true });
  await assertDisposableServer(server.url, { env: okDataPath });
  await server.close();
});

test('no server at the address is refused', async () => {
  const server = await fakeServer({});
  await server.close();
  await assert.rejects(assertDisposableServer(server.url, { env: okDataPath }), /No Mimicway answers/);
});

test('DATA_PATH unset, relative, or inside the repository is refused', async () => {
  const server = await fakeServer({ status: 'ok', disposable_data: true });
  for (const [env, message] of [
    [{}, /not set/],
    [{ DATA_PATH: './data' }, /absolute/],
    [{ DATA_PATH: path.join(repository, 'data') }, /inside the repository/],
    [{ DATA_PATH: repository }, /inside the repository/],
    [{ DATA_PATH: path.join(repository, 'frontend', '..', 'data') }, /inside the repository/],
  ]) {
    await assert.rejects(assertDisposableServer(server.url, { env }), message);
  }
  await server.close();
});

test('a directory beside the repository is not inside it', () => {
  const beside = `${repository.replace(/\/$/, '')}-data`;
  assert.equal(dataDir({ DATA_PATH: beside }), beside);
});
