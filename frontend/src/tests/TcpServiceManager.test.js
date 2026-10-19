import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import TcpServiceManager from '../lib/components/TcpServiceManager.svelte';
import { getTcpServices, getTcpStatus, createTcpService, updateTcpService, deleteTcpService } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  getTcpServices: vi.fn(),
  getTcpStatus: vi.fn(),
  createTcpService: vi.fn(),
  updateTcpService: vi.fn(),
  deleteTcpService: vi.fn(),
}));

function pingService(overrides = {}) {
  return {
    name: 'heartbeat',
    listen_port: 9000,
    rules: [{ name: 'ping', matcher: { type: 'Any' }, response_hex: '706f6e67' }],
    ...overrides,
  };
}

describe('TcpServiceManager', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows an empty state when no TCP service exists', async () => {
    getTcpServices.mockResolvedValue([]);
    getTcpStatus.mockResolvedValue([]);
    const { getByText } = render(TcpServiceManager);
    await waitFor(() => expect(getByText('No TCP service configured yet.')).toBeInTheDocument());
  });

  it('shows a service with its port, its rule count and its listening status', async () => {
    getTcpServices.mockResolvedValue([pingService()]);
    getTcpStatus.mockResolvedValue([{ name: 'heartbeat', listen_port: 9000, listening: true, error: null }]);
    const { getByTestId, getByText } = render(TcpServiceManager);

    await waitFor(() => expect(getByTestId('tcp-manager-item-heartbeat')).toBeInTheDocument());
    expect(getByText('heartbeat')).toBeInTheDocument();
    expect(getByText(':9000')).toBeInTheDocument();
    expect(getByText('1 rule')).toBeInTheDocument();
    expect(getByText('Listening')).toBeInTheDocument();
  });

  it('shows a bind failure badge, with the error as its title, when `listening` is false', async () => {
    getTcpServices.mockResolvedValue([pingService()]);
    getTcpStatus.mockResolvedValue([
      { name: 'heartbeat', listen_port: 9000, listening: false, error: 'address in use' },
    ]);
    const { getByText } = render(TcpServiceManager);

    await waitFor(() => expect(getByText('Bind failed')).toBeInTheDocument());
    expect(getByText('Bind failed').getAttribute('title')).toBe('address in use');
  });

  it('opens the creation form with one empty rule', async () => {
    getTcpServices.mockResolvedValue([]);
    getTcpStatus.mockResolvedValue([]);
    const { getByTestId } = render(TcpServiceManager);

    await waitFor(() => expect(getByTestId('tcp-manager-add-button')).toBeInTheDocument());
    await fireEvent.click(getByTestId('tcp-manager-add-button'));

    expect(getByTestId('tcp-manager-form')).toBeInTheDocument();
    expect(getByTestId('tcp-manager-rule-0')).toBeInTheDocument();
  });

  it('creating a service sends the text-mode response as hexadecimal', async () => {
    getTcpServices.mockResolvedValue([]);
    getTcpStatus.mockResolvedValue([]);
    createTcpService.mockResolvedValue(pingService());
    const { getByTestId } = render(TcpServiceManager);

    await waitFor(() => expect(getByTestId('tcp-manager-add-button')).toBeInTheDocument());
    await fireEvent.click(getByTestId('tcp-manager-add-button'));

    await fireEvent.input(getByTestId('tcp-manager-form-name-input'), { target: { value: 'heartbeat' } });
    await fireEvent.input(getByTestId('tcp-manager-form-port-input'), { target: { value: '9000' } });
    await fireEvent.input(getByTestId('tcp-manager-rule-0-name-input'), { target: { value: 'ping' } });
    await fireEvent.input(getByTestId('tcp-manager-rule-0-response-value-input'), { target: { value: 'pong' } });

    await fireEvent.click(getByTestId('tcp-manager-form-save-button'));

    await waitFor(() =>
      expect(createTcpService).toHaveBeenCalledWith({
        name: 'heartbeat',
        listen_port: 9000,
        rules: [{ name: 'ping', matcher: { type: 'Any' }, response_hex: '706f6e67' }],
      }),
    );
  });

  it('creating a service sends the text-mode prefix as hexadecimal', async () => {
    getTcpServices.mockResolvedValue([]);
    getTcpStatus.mockResolvedValue([]);
    createTcpService.mockResolvedValue(pingService());
    const { getByTestId } = render(TcpServiceManager);

    await waitFor(() => expect(getByTestId('tcp-manager-add-button')).toBeInTheDocument());
    await fireEvent.click(getByTestId('tcp-manager-add-button'));

    await fireEvent.input(getByTestId('tcp-manager-form-name-input'), { target: { value: 'svc' } });
    await fireEvent.input(getByTestId('tcp-manager-form-port-input'), { target: { value: '9001' } });
    await fireEvent.input(getByTestId('tcp-manager-rule-0-name-input'), { target: { value: 'r' } });
    await fireEvent.change(getByTestId('tcp-manager-rule-0-matcher-type-select'), { target: { value: 'Prefix' } });
    await fireEvent.input(getByTestId('tcp-manager-rule-0-matcher-value-input'), { target: { value: 'PING' } });

    await fireEvent.click(getByTestId('tcp-manager-form-save-button'));

    await waitFor(() =>
      expect(createTcpService).toHaveBeenCalledWith(
        expect.objectContaining({
          rules: [expect.objectContaining({ matcher: { type: 'Prefix', value: '50494e47' } })],
        }),
      ),
    );
  });

  it('refuses to create a service without a rule, with a clear message and no API call', async () => {
    getTcpServices.mockResolvedValue([]);
    getTcpStatus.mockResolvedValue([]);
    const { getByTestId } = render(TcpServiceManager);

    await waitFor(() => expect(getByTestId('tcp-manager-add-button')).toBeInTheDocument());
    await fireEvent.click(getByTestId('tcp-manager-add-button'));
    await fireEvent.click(getByTestId('tcp-manager-rule-0-remove-button'));

    await fireEvent.input(getByTestId('tcp-manager-form-name-input'), { target: { value: 'svc' } });
    await fireEvent.input(getByTestId('tcp-manager-form-port-input'), { target: { value: '9000' } });
    await fireEvent.click(getByTestId('tcp-manager-form-save-button'));

    await waitFor(() =>
      expect(getByTestId('tcp-manager-form-error')).toHaveTextContent(
        'At least one rule is required (otherwise no connection gets an answer).',
      ),
    );
    expect(createTcpService).not.toHaveBeenCalled();
  });

  it('shows the server error (a port already in use) and keeps the form open', async () => {
    getTcpServices.mockResolvedValue([]);
    getTcpStatus.mockResolvedValue([]);
    createTcpService.mockRejectedValue(new Error('Port 9000 is already used by another TCP service.'));
    const { getByTestId } = render(TcpServiceManager);

    await waitFor(() => expect(getByTestId('tcp-manager-add-button')).toBeInTheDocument());
    await fireEvent.click(getByTestId('tcp-manager-add-button'));
    await fireEvent.input(getByTestId('tcp-manager-form-name-input'), { target: { value: 'svc' } });
    await fireEvent.input(getByTestId('tcp-manager-form-port-input'), { target: { value: '9000' } });
    await fireEvent.input(getByTestId('tcp-manager-rule-0-name-input'), { target: { value: 'r' } });
    await fireEvent.click(getByTestId('tcp-manager-form-save-button'));

    await waitFor(() =>
      expect(getByTestId('tcp-manager-form-error')).toHaveTextContent(
        'Port 9000 is already used by another TCP service.',
      ),
    );
    expect(getByTestId('tcp-manager-form')).toBeInTheDocument();
  });

  it('pre-fills the edit form, with the name locked and hexadecimal that is valid UTF-8 shown as text', async () => {
    getTcpServices.mockResolvedValue([pingService()]);
    getTcpStatus.mockResolvedValue([]);
    const { getByTestId } = render(TcpServiceManager);

    await waitFor(() => expect(getByTestId('tcp-manager-edit-button-heartbeat')).toBeInTheDocument());
    await fireEvent.click(getByTestId('tcp-manager-edit-button-heartbeat'));

    expect(getByTestId('tcp-manager-form-name-input')).toHaveValue('heartbeat');
    expect(getByTestId('tcp-manager-form-name-input')).toBeDisabled();
    expect(getByTestId('tcp-manager-rule-0-response-value-input')).toHaveValue('pong');
    expect(getByTestId('tcp-manager-rule-0-response-mode-select')).toHaveValue('text');
  });

  it('saving an edited service calls updateTcpService with the new port', async () => {
    getTcpServices.mockResolvedValue([pingService()]);
    getTcpStatus.mockResolvedValue([]);
    updateTcpService.mockResolvedValue(pingService({ listen_port: 9500 }));
    const { getByTestId } = render(TcpServiceManager);

    await waitFor(() => expect(getByTestId('tcp-manager-edit-button-heartbeat')).toBeInTheDocument());
    await fireEvent.click(getByTestId('tcp-manager-edit-button-heartbeat'));
    await fireEvent.input(getByTestId('tcp-manager-form-port-input'), { target: { value: '9500' } });
    await fireEvent.click(getByTestId('tcp-manager-form-save-button'));

    await waitFor(() =>
      expect(updateTcpService).toHaveBeenCalledWith('heartbeat', expect.objectContaining({ listen_port: 9500 })),
    );
  });

  it('deletes a service after confirmation, not before', async () => {
    getTcpServices.mockResolvedValue([pingService()]);
    getTcpStatus.mockResolvedValue([]);
    deleteTcpService.mockResolvedValue(null);
    const onNotify = vi.fn();
    const { getByTestId, getByRole } = render(TcpServiceManager, { props: { onNotify } });

    await waitFor(() => expect(getByTestId('tcp-manager-delete-button-heartbeat')).toBeInTheDocument());
    await fireEvent.click(getByTestId('tcp-manager-delete-button-heartbeat'));

    expect(getByRole('dialog')).toBeInTheDocument();
    expect(deleteTcpService).not.toHaveBeenCalled();

    await fireEvent.click(getByTestId('confirm-dialog-confirm-button'));

    await waitFor(() => expect(deleteTcpService).toHaveBeenCalledWith('heartbeat'));
    expect(onNotify).toHaveBeenCalledWith('TCP service "heartbeat" deleted', 'success');
  });

  it('calls onBack when Back is clicked', async () => {
    getTcpServices.mockResolvedValue([]);
    getTcpStatus.mockResolvedValue([]);
    const onBack = vi.fn();
    const { getByTestId } = render(TcpServiceManager, { props: { onBack } });
    await waitFor(() => expect(getByTestId('tcp-manager-back-button')).toBeInTheDocument());
    await fireEvent.click(getByTestId('tcp-manager-back-button'));
    expect(onBack).toHaveBeenCalled();
  });
});
