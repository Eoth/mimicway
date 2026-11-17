import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ObservedCalls from '../lib/components/ObservedCalls.svelte';
import { getObservedCalls, getObservedCall } from '../lib/api.js';

vi.mock('../lib/api.js');

const failedSummary = {
  id: 9,
  timestamp: 1_700_000_000_000,
  method: 'POST',
  path: '/orders/ws',
  query: 'a=1',
  operation: 'GetStatus',
  status: null,
  failure: 'unreachable',
};
const answeredSummary = { ...failedSummary, id: 4, failure: null, status: 200, query: null };

const failedDetail = {
  id: 9,
  timestamp: 1_700_000_000_000,
  method: 'POST',
  operation: 'GetStatus',
  request: {
    path: '/orders/ws',
    query: 'a=1',
    host: 'mimic.test',
    headers: { authorization: '[redacted]', 'content-type': 'text/xml' },
    body: { text: "<GetStatus ref='1'/>", truncated: false, size: 20, utf8: true, content_type: 'text/xml' },
  },
  response: null,
  failure: {
    cause: 'unreachable',
    detail: 'Connection refused (os error 111)',
    status: 502,
    target: 'http://backend.test/ws',
  },
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe('ObservedCalls', () => {
  it('asks the server nothing until the button is pressed', async () => {
    const view = render(ObservedCalls, { props: { serviceName: 'orders' } });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(getObservedCalls).not.toHaveBeenCalled();
    expect(view.getByText('Show the calls kept')).toBeInTheDocument();
  });

  it('lists the calls kept, a failed relay said in words, with the service and its group', async () => {
    getObservedCalls.mockResolvedValue([failedSummary, answeredSummary]);
    const view = render(ObservedCalls, { props: { serviceName: 'orders', groupName: 'team-a' } });
    await fireEvent.click(view.getByText('Show the calls kept'));
    await waitFor(() => expect(view.getByText('2 calls kept.')).toBeInTheDocument());
    expect(getObservedCalls).toHaveBeenCalledWith('orders', 'team-a');
    const rows = view.container.querySelectorAll('.call-row');
    expect(rows[0].textContent).toContain('/orders/ws?a=1');
    expect(rows[0].textContent).toContain('No response: backend unreachable');
    expect(rows[1].textContent).toContain('200');
    expect(view.getByText('Refresh the calls')).toBeInTheDocument();
  });

  it('opens a failed call: its whole request, why no response came, and the command that sends it again', async () => {
    getObservedCalls.mockResolvedValue([failedSummary]);
    getObservedCall.mockResolvedValue(failedDetail);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const view = render(ObservedCalls, { props: { serviceName: 'orders' } });
    await fireEvent.click(view.getByText('Show the calls kept'));
    const toggle = await view.findByTestId('observed-call-toggle-orders-9');
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(getObservedCall).toHaveBeenCalledWith('orders', null, 9);

    const reason = await view.findByTestId('observed-call-reason-orders-9');
    expect(reason.textContent).toContain('The backend could not be reached');
    expect(view.getByText('Technical detail: Connection refused (os error 111)')).toBeInTheDocument();
    expect(view.getByTestId('observed-call-address-orders-9').textContent).toBe('POST /orders/ws?a=1');
    expect(view.getByTestId('observed-call-request-body-orders-9').textContent).toBe("<GetStatus ref='1'/>");
    const command = view.getByTestId('observed-call-command-orders-9').textContent;
    expect(command).toContain("--header 'authorization: [redacted]'");
    expect(command).toContain(`--data-raw '<GetStatus ref='\\''1'\\''/>'`);
    expect(command).toContain(`'${window.location.origin}/orders/ws?a=1'`);
    expect(view.getByText(/put the real values back/)).toBeInTheDocument();

    await fireEvent.click(view.getByTestId('observed-call-copy-button-orders-9'));
    await waitFor(() => expect(view.getByText('Command copied.')).toBeInTheDocument());
    expect(writeText).toHaveBeenCalledWith(command);

    await fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(view.getByTestId('observed-call-detail-orders-9')).not.toBeVisible();
  });

  it('says a body cut at the size limit, with its whole size and the setting, and a body that was not UTF-8', async () => {
    getObservedCalls.mockResolvedValue([answeredSummary]);
    getObservedCall.mockResolvedValue({
      ...failedDetail,
      id: 4,
      failure: null,
      request: {
        ...failedDetail.request,
        headers: {},
        body: { text: 'abc', truncated: true, size: 70000, utf8: false, content_type: null },
      },
      response: {
        status: 200,
        headers: {},
        body: { text: '', truncated: false, size: 0, utf8: true, content_type: null },
      },
    });
    const view = render(ObservedCalls, { props: { serviceName: 'orders' } });
    await fireEvent.click(view.getByText('Show the calls kept'));
    await fireEvent.click(await view.findByTestId('observed-call-toggle-orders-4'));
    const cut = await view.findByTestId('observed-call-request-body-orders-4-truncated');
    expect(cut.textContent).toContain('70000 bytes');
    expect(cut.textContent).toContain('TRAFFIC_OBSERVATION_MAX_BODY_SIZE');
    expect(view.getByTestId('observed-call-request-body-orders-4-non-utf8')).toBeInTheDocument();
    expect(view.getByText('The body was cut: the command sends only the part kept.')).toBeInTheDocument();
    expect(view.getByText('Response 200')).toBeInTheDocument();
    expect(view.getByText('No body.')).toBeInTheDocument();
    expect(view.queryByText(/put the real values back/)).not.toBeInTheDocument();
  });

  it('says when copying is not available, and leaves the command to select', async () => {
    getObservedCalls.mockResolvedValue([failedSummary]);
    getObservedCall.mockResolvedValue(failedDetail);
    Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
    const view = render(ObservedCalls, { props: { serviceName: 'orders' } });
    await fireEvent.click(view.getByText('Show the calls kept'));
    await fireEvent.click(await view.findByTestId('observed-call-toggle-orders-9'));
    await fireEvent.click(await view.findByTestId('observed-call-copy-button-orders-9'));
    await waitFor(() => expect(view.getByText(/Copying is not available/)).toBeInTheDocument());
  });
  it('downloads the request body as kept, as a file to save whatever media type the caller sent', async () => {
    getObservedCalls.mockResolvedValue([failedSummary]);
    getObservedCall.mockResolvedValue({
      ...failedDetail,
      request: { ...failedDetail.request, body: { ...failedDetail.request.body, content_type: 'text/html' } },
    });
    let saved = null;
    URL.createObjectURL = vi.fn((blob) => {
      saved = blob;
      return 'blob:kept';
    });
    URL.revokeObjectURL = vi.fn();
    const clicked = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function () {
      expect(this.download).toBe('request-body-9.txt');
      expect(this.getAttribute('href')).toBe('blob:kept');
    });
    const view = render(ObservedCalls, { props: { serviceName: 'orders' } });
    await fireEvent.click(view.getByText('Show the calls kept'));
    await fireEvent.click(await view.findByTestId('observed-call-toggle-orders-9'));
    await fireEvent.click(await view.findByTestId('observed-call-download-button-orders-9'));
    expect(clicked).toHaveBeenCalledTimes(1);
    expect(saved.type).toBe('application/octet-stream');
    expect(await saved.text()).toBe("<GetStatus ref='1'/>");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:kept');
    clicked.mockRestore();
  });
  it('sends a body with control characters from its file, and offers no command for a header holding one', async () => {
    getObservedCalls.mockResolvedValue([failedSummary, { ...failedSummary, id: 10 }]);
    getObservedCall.mockImplementation(async (_name, _group, id) =>
      id === 9
        ? {
            ...failedDetail,
            request: {
              ...failedDetail.request,
              body: { ...failedDetail.request.body, text: 'x\u001b[201~\u0015id\r' },
            },
          }
        : { ...failedDetail, id: 10, request: { ...failedDetail.request, headers: { 'x-a': 'v\u001b' } } },
    );
    const view = render(ObservedCalls, { props: { serviceName: 'orders' } });
    await fireEvent.click(view.getByText('Show the calls kept'));
    await fireEvent.click(await view.findByTestId('observed-call-toggle-orders-9'));
    const command = (await view.findByTestId('observed-call-command-orders-9')).textContent;
    expect(command).toContain('--data-binary @request-body-9.xml');
    expect(command).not.toContain('\u001b');
    expect(view.getByTestId('observed-call-body-file-orders-9').textContent).toContain('request-body-9.xml');

    await fireEvent.click(view.getByTestId('observed-call-toggle-orders-10'));
    expect((await view.findByTestId('observed-call-no-command-orders-10')).textContent).toContain('control character');
    expect(view.queryByTestId('observed-call-command-orders-10')).not.toBeInTheDocument();
    expect(view.queryByTestId('observed-call-copy-button-orders-10')).not.toBeInTheDocument();
  });
  it('says a call cut outside its body, and offers no command for it', async () => {
    getObservedCalls.mockResolvedValue([failedSummary]);
    getObservedCall.mockResolvedValue({ ...failedDetail, request: { ...failedDetail.request, head_truncated: true } });
    const view = render(ObservedCalls, { props: { serviceName: 'orders' } });
    await fireEvent.click(view.getByText('Show the calls kept'));
    await fireEvent.click(await view.findByTestId('observed-call-toggle-orders-9'));
    expect((await view.findByTestId('observed-call-request-cut-orders-9')).textContent).toContain('16 KiB');
    expect(view.getByTestId('observed-call-no-command-orders-9').textContent).toContain('cut at the size limit');
    expect(view.queryByTestId('observed-call-copy-button-orders-9')).not.toBeInTheDocument();
  });
});
