import { describe, it, expect } from 'vitest';
import { buildServiceTestUrl } from '../lib/service-url.js';

// Source unique de verite pour l'URL de test, partagee par ServiceCard.svelte
// (vue liste) et ServiceForm.svelte (ajout/edition) — un seul test couvre
// les deux points d'usage plutot que de dupliquer l'assertion.
describe('buildServiceTestUrl', () => {
  it('sans groupe : prefixe uniquement par le nom du service', () => {
    expect(buildServiceTestUrl({ name: 'svc-users', listenPath: '/v1/*' })).toBe('/svc-users/v1/*');
  });

  it('avec groupe : prefixe par le code du groupe puis le nom du service', () => {
    expect(buildServiceTestUrl({ name: 'svc-users', listenPath: '/v1/*', groupCode: 'ab3f9' })).toBe(
      '/ab3f9/svc-users/v1/*',
    );
  });

  it('chemin vide retombe sur le wildcard /*', () => {
    expect(buildServiceTestUrl({ name: 'svc-users', listenPath: '' })).toBe('/svc-users/*');
  });

  it('chemin sans slash initial est normalise', () => {
    expect(buildServiceTestUrl({ name: 'svc-users', listenPath: 'v1/*' })).toBe('/svc-users/v1/*');
  });

  it('nom vide retombe sur un placeholder "..."', () => {
    expect(buildServiceTestUrl({ name: '', listenPath: '/v1/*' })).toBe('/.../v1/*');
  });

  it('avec baseUrl (usage ServiceForm) : prefixe l URL complete', () => {
    expect(
      buildServiceTestUrl({
        name: 'svc-users',
        listenPath: '/v1/*',
        groupCode: 'ab3f9',
        baseUrl: 'http://localhost:7342',
      }),
    ).toBe('http://localhost:7342/ab3f9/svc-users/v1/*');
  });
});
