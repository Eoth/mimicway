import { describe, it, expect, beforeEach, vi } from 'vitest';
import { loadRuntimeConfig, getApiBaseUrl, _resetForTests } from '../lib/runtime-config.js';

// `getApiBaseUrl()` stays '' (relative URLs) until /runtime-config.json gives a usable value. The setting is read at
// run time, not build time, so that one build can point at a different API in each deployment.
describe('runtime-config', () => {
  beforeEach(() => {
    _resetForTests();
    vi.unstubAllGlobals();
  });

  it('starts with an empty base URL (relative paths)', () => {
    expect(getApiBaseUrl()).toBe('');
  });

  it('adopts the URL given by /runtime-config.json', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ api_base_url: 'https://api.example.com' }),
      }),
    );

    await loadRuntimeConfig();

    expect(getApiBaseUrl()).toBe('https://api.example.com');
    expect(fetch).toHaveBeenCalledWith('/runtime-config.json');
  });

  it('drops a trailing slash from the configured URL', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ api_base_url: 'https://api.example.com/' }),
      }),
    );

    await loadRuntimeConfig();

    expect(getApiBaseUrl()).toBe('https://api.example.com');
  });

  it('stays empty when `api_base_url` is missing', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({}),
      }),
    );

    await loadRuntimeConfig();

    expect(getApiBaseUrl()).toBe('');
  });

  it('stays empty when the HTTP answer is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }));

    await loadRuntimeConfig();

    expect(getApiBaseUrl()).toBe('');
  });

  it('stays empty and does not throw when fetch fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network error')));

    await expect(loadRuntimeConfig()).resolves.toBeUndefined();
    expect(getApiBaseUrl()).toBe('');
  });

  it('stays empty and does not throw when the answer is not valid JSON', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => {
          throw new Error('invalid json');
        },
      }),
    );

    await expect(loadRuntimeConfig()).resolves.toBeUndefined();
    expect(getApiBaseUrl()).toBe('');
  });
});
