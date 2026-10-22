import { render, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import App from '../App.svelte';
import * as api from '../lib/api.js';
import { setAuth, logout } from '../lib/auth.svelte.js';

vi.mock('../lib/api.js', () => ({
  getServices: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn().mockResolvedValue({ services: [], groups: [] }),
  putConfig: vi.fn(),
  toggleService: vi.fn(),
  createService: vi.fn(),
  updateService: vi.fn(),
  resetConfig: vi.fn(),
  getAuthStatus: vi.fn(),
  validateToken: vi.fn(),
  getGroups: vi.fn().mockResolvedValue([]),
  createGroup: vi.fn(),
}));

describe('App: the reset button follows SHOW_RESET_BUTTON', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getServices.mockResolvedValue([]);
    api.getGroups.mockResolvedValue([]);
    window.matchMedia = window.matchMedia || vi.fn().mockReturnValue({ matches: false });
  });

  it('hides the reset button when authentication is off and `show_reset_button` is false', async () => {
    api.getAuthStatus.mockResolvedValue({ enabled: false, show_reset_button: false });
    const { queryByTitle } = render(App);
    await waitFor(() => expect(api.getServices).toHaveBeenCalled());
    expect(queryByTitle('Remove every service')).not.toBeInTheDocument();
  });

  it('shows the reset button when authentication is off and `show_reset_button` is true', async () => {
    api.getAuthStatus.mockResolvedValue({ enabled: false, show_reset_button: true });
    const { queryByTitle } = render(App);
    await waitFor(() => expect(queryByTitle('Remove every service')).toBeInTheDocument());
  });
});

// Restoring a backup is for super-admins only on the server (require_super_admin): the view must not be offered to
// a user it would refuse.
describe('App: the backups view is offered to whoever may restore one', () => {
  const backupsButton = (container) => container.querySelector('[data-testid="app-nav-backups-button"]');

  beforeEach(() => {
    vi.clearAllMocks();
    logout();
    api.getServices.mockResolvedValue([]);
    api.getGroups.mockResolvedValue([]);
    window.matchMedia = window.matchMedia || vi.fn().mockReturnValue({ matches: false });
  });

  async function renderSignedIn(isSuperAdmin) {
    setAuth({ access_token: 't', username: 'u', is_super_admin: isSuperAdmin });
    api.getAuthStatus.mockResolvedValue({ enabled: true, show_reset_button: false });
    const view = render(App);
    await waitFor(() => expect(api.getServices).toHaveBeenCalled());
    return view;
  }

  it('hides it from a signed-in user who is not a super-admin', async () => {
    const { container } = await renderSignedIn(false);
    expect(backupsButton(container)).toBeNull();
  });

  it('shows it to a super-admin', async () => {
    const { container } = await renderSignedIn(true);
    expect(backupsButton(container)).not.toBeNull();
  });

  it('shows it when authentication is off, even with the reset button hidden', async () => {
    api.getAuthStatus.mockResolvedValue({ enabled: false, show_reset_button: false });
    const { container } = render(App);
    await waitFor(() => expect(api.getServices).toHaveBeenCalled());
    expect(backupsButton(container)).not.toBeNull();
  });
});
