import { render, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RequestLog from '../lib/components/RequestLog.svelte';
import { getLogs } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  getLogs: vi.fn(),
}));

describe('RequestLog date and time', () => {
  it('shows the date and the time of the request, not the time alone', async () => {
    const ts = new Date('2026-01-15T10:30:00').getTime();
    getLogs.mockResolvedValue([
      {
        timestamp: ts,
        service_name: 'svc-a',
        method: 'GET',
        path: '/svc-a/foo',
        mode: 'mock',
        rule_matched: 'r1',
        target_url: null,
        status: 200,
      },
    ]);

    const { getByText, container } = render(RequestLog);

    await waitFor(() => expect(getByText('svc-a')).toBeInTheDocument());

    // English has no regional date format of its own: dates follow the browser's locale (intlLocale()), the French
    // order is checked by french.test.js.
    const expected = new Date(ts).toLocaleString(undefined, {
      day: '2-digit',
      month: '2-digit',
      year: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
    const cell = container.querySelector('.col-time');
    expect(cell.textContent).toBe(expected);
  });

  it('shows the "Date/time" column header', async () => {
    getLogs.mockResolvedValue([
      {
        timestamp: Date.now(),
        service_name: 'svc-a',
        method: 'GET',
        path: '/svc-a/foo',
        mode: 'mock',
        rule_matched: 'r1',
        target_url: null,
        status: 200,
      },
    ]);
    const { getByText } = render(RequestLog);
    await waitFor(() => expect(getByText('Date/time')).toBeInTheDocument());
  });
});

describe('RequestLog: a response sent as server-sent events', () => {
  const entry = (stream) => ({
    timestamp: Date.now(),
    service_name: 'feed',
    method: 'GET',
    path: '/feed/log/42',
    mode: 'mock',
    rule_matched: 'log',
    target_url: null,
    status: 200,
    stream,
  });

  it('shows how far the stream got and the events it sent, in order', async () => {
    getLogs.mockResolvedValue([
      entry({
        planned: 3,
        sent_count: 2,
        sent: [{ data: 'pulling image' }, { event: 'progress', data: 'copied 3\nfailed: 0' }],
        sent_truncated: false,
        outcome: 'cut',
      }),
    ]);
    const { getByTestId } = render(RequestLog);
    await waitFor(() => expect(getByTestId('request-log-detail-button-0')).toBeInTheDocument());
    getByTestId('request-log-detail-button-0').click();

    await waitFor(() => expect(getByTestId('request-log-detail-stream')).toBeInTheDocument());
    expect(getByTestId('request-log-detail-stream')).toHaveTextContent('2 of 3 events sent, cut by its rule');
    const items = [...getByTestId('request-log-detail-stream-events').querySelectorAll('li')];
    expect(items.map((li) => li.textContent.trim().replace(/\s+/g, ' '))).toEqual([
      'pulling image',
      'progress copied 3 failed: 0',
    ]);
  });

  it('shows no stream line for an ordinary response', async () => {
    getLogs.mockResolvedValue([entry(undefined)]);
    const { getByTestId, queryByTestId } = render(RequestLog);
    await waitFor(() => expect(getByTestId('request-log-detail-button-0')).toBeInTheDocument());
    getByTestId('request-log-detail-button-0').click();
    await waitFor(() => expect(getByTestId('request-log-detail-modal')).toBeInTheDocument());
    expect(queryByTestId('request-log-detail-stream')).not.toBeInTheDocument();
  });
});
