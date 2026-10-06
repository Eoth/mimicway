import { describe, it, expect, beforeEach, vi } from 'vitest';
import { getServices } from '../lib/api.js';
import { loadRuntimeConfig, _resetForTests } from '../lib/runtime-config.js';

// Verifie que api.js compose reellement ses appels avec l'URL de base
// resolue par runtime-config.js (defaut '' -> chemin relatif, ou l'URL
// fournie par /runtime-config.json) — le point de raccordement concret pour
// rendre l'URL de l'API configurable independamment du Host du frontend.
describe('api.js — resolution de l URL de base', () => {
  beforeEach(() => {
    _resetForTests();
    vi.unstubAllGlobals();
  });

  it('appelle un chemin relatif par defaut (comportement historique inchange)', async () => {
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

  it('prefixe les appels par l URL configuree via /runtime-config.json', async () => {
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
