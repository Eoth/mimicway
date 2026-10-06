import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import UrlHealthBadge from '../lib/components/UrlHealthBadge.svelte';
import { pingService } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  pingService: vi.fn(),
}));

describe('UrlHealthBadge', () => {
  it('affiche "Non testé" avant tout test', () => {
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-a' } });
    expect(getByText('Not tested')).toBeInTheDocument();
  });

  it('teste la cible au clic et affiche "Accessible" si joignable', async () => {
    pingService.mockResolvedValue({ reachable: true, checked_at: Date.now(), error: null });
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-a' } });

    await fireEvent.click(getByText('Test the target (network only)'));

    await waitFor(() => expect(getByText('Reachable')).toBeInTheDocument());
    expect(pingService).toHaveBeenCalledWith('svc-a', null);
  });

  it('affiche "Inaccessible" et un avertissement si la cible ne repond pas', async () => {
    pingService.mockResolvedValue({ reachable: false, checked_at: Date.now(), error: 'connection refused' });
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-b' } });

    await fireEvent.click(getByText('Test the target (network only)'));

    await waitFor(() => expect(getByText('Unreachable')).toBeInTheDocument());
    expect(getByText(/^Only the mock mode can be used for this service/)).toBeInTheDocument();
  });

  it("affiche une erreur si l'appel echoue", async () => {
    pingService.mockRejectedValue(new Error('502 Bad Gateway'));
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-c' } });

    await fireEvent.click(getByText('Test the target (network only)'));

    await waitFor(() => expect(getByText('502 Bad Gateway')).toBeInTheDocument());
  });

  it("mentionne explicitement qu'il s'agit d'un test reseau, pas applicatif", () => {
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-d' } });
    expect(getByText(/network only/)).toBeInTheDocument();
  });

  it('affiche "Expiré" quand le dernier test date de plus de PING_TTL_MS', async () => {
    vi.useFakeTimers();
    pingService.mockResolvedValue({ reachable: true, checked_at: Date.now(), error: null });
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-e' } });

    await fireEvent.click(getByText('Test the target (network only)'));
    await vi.waitFor(() => expect(getByText('Reachable')).toBeInTheDocument());

    await vi.advanceTimersByTimeAsync(130_000);

    expect(getByText('Expired')).toBeInTheDocument();
    vi.useRealTimers();
  });
});

describe('UrlHealthBadge: the lifetime of a result', () => {
  it('expires a result when the server forgets it (PING_TTL_MS of src/server/ping.rs)', () => {
    const constant = (file, pattern) =>
      Number(readFileSync(join(__dirname, file), 'utf8').match(pattern)[1].replaceAll('_', ''));
    const server = constant('../../../src/server/ping.rs', /pub const PING_TTL_MS: u64 = ([\d_]+);/);
    const badge = constant('../lib/components/UrlHealthBadge.svelte', /const PING_TTL_MS = ([\d_]+);/);
    expect(badge).toBe(server);
  });
});
