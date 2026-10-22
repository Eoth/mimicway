import { render, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import App from '../App.svelte';
import * as api from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  getServices: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn().mockResolvedValue({ services: [], groups: [] }),
  putConfig: vi.fn(),
  toggleService: vi.fn(),
  createService: vi.fn(),
  updateService: vi.fn(),
  resetConfig: vi.fn(),
  getAuthStatus: vi.fn().mockResolvedValue({ enabled: false, show_reset_button: false }),
  validateToken: vi.fn(),
  getGroups: vi.fn().mockResolvedValue([]),
  createGroup: vi.fn(),
  getMessagingStatus: vi.fn().mockRejectedValue(new Error('404 Not Found')),
  getTcpStatus: vi.fn(),
}));

describe('App: the raw TCP mock button follows the `tcp-mock` feature', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getServices.mockResolvedValue([]);
    api.getGroups.mockResolvedValue([]);
    api.getAuthStatus.mockResolvedValue({ enabled: false, show_reset_button: false });
    api.getMessagingStatus.mockRejectedValue(new Error('404 Not Found'));
    window.matchMedia = window.matchMedia || vi.fn().mockReturnValue({ matches: false });
  });

  it('hides the button when /api/tcp/status answers 404 (binary built without the feature)', async () => {
    api.getTcpStatus.mockRejectedValue(new Error('404 Not Found'));
    const { queryByTitle } = render(App);
    await waitFor(() => expect(api.getServices).toHaveBeenCalled());
    expect(queryByTitle('Raw TCP mock')).not.toBeInTheDocument();
  });

  it('hides the button when /api/tcp/status answers 403 (a user who is not a super-admin)', async () => {
    api.getTcpStatus.mockRejectedValue(new Error('403 Forbidden'));
    const { queryByTitle } = render(App);
    await waitFor(() => expect(api.getServices).toHaveBeenCalled());
    expect(queryByTitle('Raw TCP mock')).not.toBeInTheDocument();
  });

  it('shows the button when /api/tcp/status answers, even with an empty list', async () => {
    api.getTcpStatus.mockResolvedValue([]);
    const { queryByTitle } = render(App);
    await waitFor(() => expect(queryByTitle('Raw TCP mock')).toBeInTheDocument());
  });
});
