import { describe, it, expect, beforeEach, vi } from 'vitest';
import { getServices } from '../lib/api.js';
import { loadRuntimeConfig, _resetForTests } from '../lib/runtime-config.js';

// `api.js` builds its calls on the base URL from `runtime-config.js`: relative by default, or the URL given by
// /runtime-config.json, which lets the API live on another host than the UI.
describe('api.js - base URL', () => {
  beforeEach(() => {
    _resetForTests();
    vi.unstubAllGlobals();
  });

  it('calls a relative path by default', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => [],
      }),
    );

    await getServices();

    expect(fetch).toHaveBeenCalledWith('/api/services', expect.any(Object));
  });

  it('prefixes the calls with the URL from /runtime-config.json', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValueOnce({
          ok: true,
          status: 200,
          json: async () => ({ api_base_url: 'https://api.example.com' }),
        })
        .mockResolvedValueOnce({ ok: true, status: 200, json: async () => [] }),
    );

    await loadRuntimeConfig();
    await getServices();

    expect(fetch).toHaveBeenLastCalledWith('https://api.example.com/api/services', expect.any(Object));
  });
});
