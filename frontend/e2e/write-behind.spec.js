import { test, expect } from './fixtures.mjs';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { dataDir } from './disposable-server.mjs';

const API = 'http://localhost:7342/api';

// The store applies a change in memory at once and writes mock-config.yaml in a background task (write-behind,
// src/store/mod.rs). The test reads that file rather than the API, which answers from memory: the file is what a
// restart would load (load_or_init), without stopping the server the whole suite shares.
const CONFIG_FILE = resolve(dataDir(), 'mock-config.yaml');

function readConfigFromDisk() {
  return readFileSync(CONFIG_FILE, 'utf-8');
}

function validService(name, overrides = {}) {
  return {
    name,
    listen_path: '/v1/*',
    real_target_url: 'http://backend:8080',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
    ...overrides,
  };
}

test.describe('Write-behind: persistence after a simulated store restart', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('a service deleted through the API disappears from the on-disk config once persisted', async ({ request }) => {
    await request.post(`${API}/services`, { data: validService('write-behind-delete-svc') });

    await expect(async () => {
      expect(readConfigFromDisk()).toContain('write-behind-delete-svc');
    }).toPass({ timeout: 5000 });

    const del = await request.delete(`${API}/services/write-behind-delete-svc`);
    expect(del.status()).toBe(204);

    await expect(async () => {
      expect(readConfigFromDisk()).not.toContain('write-behind-delete-svc');
    }).toPass({ timeout: 5000 });
  });
});
