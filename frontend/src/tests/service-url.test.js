import { describe, it, expect } from 'vitest';
import { buildServiceTestUrl } from '../lib/service-url.js';

// The one place that builds the test URL, shared by the list, the page of a service and its form: testing it here
// covers all three.
describe('buildServiceTestUrl', () => {
  it('without a group: prefixed with the service name only', () => {
    expect(buildServiceTestUrl({ name: 'svc-users', listenPath: '/v1/*' })).toBe('/svc-users/v1/*');
  });

  it('with a group: prefixed with the group code, then the service name', () => {
    expect(buildServiceTestUrl({ name: 'svc-users', listenPath: '/v1/*', groupCode: 'ab3f9' })).toBe(
      '/ab3f9/svc-users/v1/*',
    );
  });

  it('an empty path falls back to the /* wildcard', () => {
    expect(buildServiceTestUrl({ name: 'svc-users', listenPath: '' })).toBe('/svc-users/*');
  });

  it('a path without a leading slash gets one', () => {
    expect(buildServiceTestUrl({ name: 'svc-users', listenPath: 'v1/*' })).toBe('/svc-users/v1/*');
  });

  it('an empty name falls back to a "..." placeholder', () => {
    expect(buildServiceTestUrl({ name: '', listenPath: '/v1/*' })).toBe('/.../v1/*');
  });

  it('with `baseUrl`: prefixes the full URL', () => {
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
