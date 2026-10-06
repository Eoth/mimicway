import { describe, it, expect, beforeEach, vi } from 'vitest';
import { loadRuntimeConfig, getApiBaseUrl, _resetForTests } from '../lib/runtime-config.js';

// Verifie la logique de resolution de l'URL de l'API (defaut vs configure) :
// getApiBaseUrl() est '' tant que /runtime-config.json n'a jamais ete lu ou
// n'a rien fourni d'exploitable, et refletera la valeur configuree sinon.
// Config runtime plutot que build-time : une meme image Docker doit
// pouvoir pointer vers des URLs d'API differentes selon l'environnement de
// deploiement.
describe('runtime-config', () => {
  beforeEach(() => {
    _resetForTests();
    vi.unstubAllGlobals();
  });

  it('demarre avec une URL de base vide (comportement historique : chemin relatif)', () => {
    expect(getApiBaseUrl()).toBe('');
  });

  it('adopte l URL configuree renvoyee par /runtime-config.json', async () => {
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

  it('retire un slash final de l URL configuree', async () => {
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

  it('reste vide (comportement par defaut) si api_base_url est absent ou vide', async () => {
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

  it('reste vide si la reponse HTTP n est pas ok (jamais bloquant)', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }));

    await loadRuntimeConfig();

    expect(getApiBaseUrl()).toBe('');
  });

  it('reste vide si fetch echoue (backend injoignable, jamais bloquant)', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network error')));

    await expect(loadRuntimeConfig()).resolves.toBeUndefined();
    expect(getApiBaseUrl()).toBe('');
  });

  it('reste vide si la reponse n est pas un JSON valide (jamais bloquant)', async () => {
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
