import { render, waitFor, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import App from '../App.svelte';
import * as api from '../lib/api.js';
import { setAuth, logout } from '../lib/auth.svelte.js';

vi.mock('../lib/api.js', () => ({
  getServices: vi.fn().mockResolvedValue([]),
  getAuthStatus: vi.fn(),
  validateToken: vi.fn(),
  getGroups: vi.fn().mockResolvedValue([]),
  getVersion: vi.fn(),
  login: vi.fn(),
}));

// The foot of the page says which version of Mimicway answers, as the server tells it, and only to someone the
// server lets in.
describe('App: the foot of the page shows the version of the server', () => {
  const shownVersion = (container) => container.querySelector('[data-testid="app-version"]');

  beforeEach(() => {
    vi.clearAllMocks();
    logout();
    api.getServices.mockResolvedValue([]);
    api.getGroups.mockResolvedValue([]);
    api.getAuthStatus.mockResolvedValue({ enabled: false, show_reset_button: false });
    window.matchMedia = window.matchMedia || vi.fn().mockReturnValue({ matches: false });
  });

  it('shows the version the server gives', async () => {
    api.getVersion.mockResolvedValue({ version: '1.2.3' });
    const { container } = render(App);
    await waitFor(() => expect(shownVersion(container)).toHaveTextContent(/^Version 1\.2\.3$/));
  });

  it('shows nothing, and no error, when the server does not tell its version', async () => {
    api.getVersion.mockRejectedValue(new Error('Not found'));
    const { container } = render(App);
    await waitFor(() => expect(api.getServices).toHaveBeenCalled());
    await waitFor(() => expect(api.getVersion).toHaveBeenCalled());
    expect(shownVersion(container)).not.toBeInTheDocument();
    expect(container.querySelector('footer')).not.toBeInTheDocument();
    expect(container.querySelector('[role="alert"]')).not.toBeInTheDocument();
  });

  it.each([[{}], [{ version: 7 }], [{ version: '' }], [null]])('shows nothing for the answer %j', async (answer) => {
    api.getVersion.mockResolvedValue(answer);
    const { container } = render(App);
    await waitFor(() => expect(api.getVersion).toHaveBeenCalled());
    await waitFor(() => expect(api.getServices).toHaveBeenCalled());
    expect(shownVersion(container)).not.toBeInTheDocument();
  });

  it('does not ask before sign-in, asks at sign-in through the form, and again at the next sign-in', async () => {
    api.getAuthStatus.mockResolvedValue({ enabled: true, show_reset_button: false });
    api.getVersion.mockResolvedValue({ version: '1.2.3' });
    api.login.mockResolvedValue({ access_token: 't', username: 'u', is_super_admin: false });
    const { container } = render(App);
    const signIn = async () => {
      await waitFor(() => expect(container.querySelector('form.login-form')).toBeInTheDocument());
      const field = (name) => container.querySelector(`[data-testid="login-form-${name}-input"]`);
      field('username').value = 'u';
      await fireEvent.input(field('username'));
      field('password').value = 'p';
      await fireEvent.input(field('password'));
      await fireEvent.submit(container.querySelector('form.login-form'));
    };

    await waitFor(() => expect(container.querySelector('form.login-form')).toBeInTheDocument());
    expect(api.getVersion).not.toHaveBeenCalled();
    expect(shownVersion(container)).not.toBeInTheDocument();

    await signIn();
    await waitFor(() => expect(shownVersion(container)).toHaveTextContent('Version 1.2.3'));
    expect(api.getVersion).toHaveBeenCalledTimes(1);

    // The server is replaced while the page stays open; the session ends (as a 401 ends it: the module's logout,
    // not the button's handler), and the next sign-in names the server as it is now.
    api.getVersion.mockResolvedValue({ version: '1.2.4' });
    logout();
    await signIn();
    await waitFor(() => expect(shownVersion(container)).toHaveTextContent('Version 1.2.4'));
    expect(api.getVersion).toHaveBeenCalledTimes(2);
  });

  it('asks at start for someone already signed in', async () => {
    api.getAuthStatus.mockResolvedValue({ enabled: true, show_reset_button: false });
    api.getVersion.mockResolvedValue({ version: '1.2.3' });
    setAuth({ access_token: 't', username: 'u', is_super_admin: false });
    const { container } = render(App);
    await waitFor(() => expect(shownVersion(container)).toHaveTextContent('Version 1.2.3'));
    expect(api.getVersion).toHaveBeenCalledTimes(1);
  });
});
