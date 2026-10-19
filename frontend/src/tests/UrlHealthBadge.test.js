import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { afterEach, describe, it, expect, vi } from 'vitest';
import UrlHealthBadge from '../lib/components/UrlHealthBadge.svelte';
import { pingService } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  pingService: vi.fn(),
}));

// A failed assertion skips the end of its test: real timers come back here, not there, so that the next tests
// never run on fake ones.
afterEach(() => {
  vi.useRealTimers();
});

describe('UrlHealthBadge', () => {
  it('shows "Not tested" before any test', () => {
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-a' } });
    expect(getByText('Not tested')).toBeInTheDocument();
  });

  it('tests the target on click and shows "Reachable" when it answers', async () => {
    pingService.mockResolvedValue({ reachable: true, checked_at: Date.now(), error: null });
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-a' } });

    await fireEvent.click(getByText('Test the target (network only)'));

    await waitFor(() => expect(getByText('Reachable')).toBeInTheDocument());
    expect(pingService).toHaveBeenCalledWith('svc-a', null);
  });

  it('shows "Unreachable" and a warning when the target does not answer', async () => {
    pingService.mockResolvedValue({ reachable: false, checked_at: Date.now(), error: 'connection refused' });
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-b' } });

    await fireEvent.click(getByText('Test the target (network only)'));

    await waitFor(() => expect(getByText('Unreachable')).toBeInTheDocument());
    expect(getByText(/^Only the mock mode can be used for this service/)).toBeInTheDocument();
  });

  it('shows the error when the call fails', async () => {
    pingService.mockRejectedValue(new Error('502 Bad Gateway'));
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-c' } });

    await fireEvent.click(getByText('Test the target (network only)'));

    await waitFor(() => expect(getByText('502 Bad Gateway')).toBeInTheDocument());
  });

  it('says the test checks the network only', () => {
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-d' } });
    expect(getByText(/network only/)).toBeInTheDocument();
  });

  it('shows "Expired" once the last test is older than `PING_TTL_MS`', async () => {
    vi.useFakeTimers();
    pingService.mockResolvedValue({ reachable: true, checked_at: Date.now(), error: null });
    const { getByText } = render(UrlHealthBadge, { props: { serviceName: 'svc-e' } });

    await fireEvent.click(getByText('Test the target (network only)'));
    await vi.waitFor(() => expect(getByText('Reachable')).toBeInTheDocument());

    await vi.advanceTimersByTimeAsync(130_000);

    expect(getByText('Expired')).toBeInTheDocument();
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
