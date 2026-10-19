import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import BackupManager from '../lib/components/BackupManager.svelte';
import { getBackups, restoreBackup } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  getBackups: vi.fn(),
  restoreBackup: vi.fn(),
}));

describe('BackupManager', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows an empty state when there is no backup', async () => {
    getBackups.mockResolvedValue([]);
    const { getByText } = render(BackupManager);
    await waitFor(() => expect(getByText('No backup available yet.')).toBeInTheDocument());
  });

  it('lists the backups with their name, size and protected badge', async () => {
    getBackups.mockResolvedValue([
      {
        filename: 'mock-config-1690000000000-000001.yaml',
        protected: false,
        size_bytes: 2048,
        created_at_ms: 1690000000000,
      },
      { filename: 'pre-reset-1690000000000.yaml', protected: true, size_bytes: 512, created_at_ms: 1690000000000 },
    ]);
    const { getByText, container } = render(BackupManager);

    await waitFor(() => expect(getByText('mock-config-1690000000000-000001.yaml')).toBeInTheDocument());
    expect(getByText('pre-reset-1690000000000.yaml')).toBeInTheDocument();
    expect(getByText('Protected (before a reset)')).toBeInTheDocument();

    const metas = container.querySelectorAll('.backup-meta');
    expect(metas[0].textContent).toContain('2.0 KB');
    expect(metas[1].textContent).toContain('512 B');
  });

  it('opens the confirmation on Restore without calling the API yet', async () => {
    getBackups.mockResolvedValue([
      { filename: 'mock-config-1-000001.yaml', protected: false, size_bytes: 100, created_at_ms: 1690000000000 },
    ]);
    const { getByText, getByRole } = render(BackupManager);

    await waitFor(() => expect(getByText('mock-config-1-000001.yaml')).toBeInTheDocument());
    await fireEvent.click(getByText('Restore'));

    expect(getByRole('dialog')).toBeInTheDocument();
    expect(restoreBackup).not.toHaveBeenCalled();
  });

  it('calls `restoreBackup` and reloads the list once confirmed', async () => {
    getBackups.mockResolvedValue([
      { filename: 'mock-config-1-000001.yaml', protected: false, size_bytes: 100, created_at_ms: 1690000000000 },
    ]);
    restoreBackup.mockResolvedValue(null);
    const onNotify = vi.fn();
    const { getByText } = render(BackupManager, { props: { onNotify } });

    await waitFor(() => expect(getByText('mock-config-1-000001.yaml')).toBeInTheDocument());
    await fireEvent.click(getByText('Restore'));

    const keywordInput = document.getElementById('confirm-keyword-input');
    await fireEvent.input(keywordInput, { target: { value: 'RESTORE' } });
    await waitFor(() => expect(getByText('Restore', { selector: '.btn-danger' })).not.toBeDisabled());
    await fireEvent.click(getByText('Restore', { selector: '.btn-danger' }));

    await waitFor(() => expect(restoreBackup).toHaveBeenCalledWith('mock-config-1-000001.yaml'));
    await waitFor(() => expect(getBackups).toHaveBeenCalledTimes(2));
    expect(onNotify).toHaveBeenCalledWith('Configuration restored from "mock-config-1-000001.yaml"', 'success');
  });

  it('cancels without calling the API', async () => {
    getBackups.mockResolvedValue([
      { filename: 'mock-config-1-000001.yaml', protected: false, size_bytes: 100, created_at_ms: 1690000000000 },
    ]);
    const { getByText } = render(BackupManager);

    await waitFor(() => expect(getByText('mock-config-1-000001.yaml')).toBeInTheDocument());
    await fireEvent.click(getByText('Restore'));
    await fireEvent.click(getByText('Cancel'));

    expect(restoreBackup).not.toHaveBeenCalled();
  });

  it('reports an error when loading the backups is refused', async () => {
    getBackups.mockRejectedValue(new Error('Access denied'));
    const onNotify = vi.fn();
    render(BackupManager, { props: { onNotify } });

    await waitFor(() => expect(onNotify).toHaveBeenCalledWith(expect.stringContaining('Access denied'), 'error'));
  });

  it('reports an error when the restore is refused', async () => {
    getBackups.mockResolvedValue([
      { filename: 'mock-config-1-000001.yaml', protected: false, size_bytes: 100, created_at_ms: 1690000000000 },
    ]);
    restoreBackup.mockRejectedValue(new Error('Access denied'));
    const onNotify = vi.fn();
    const { getByText } = render(BackupManager, { props: { onNotify } });

    await waitFor(() => expect(getByText('mock-config-1-000001.yaml')).toBeInTheDocument());
    await fireEvent.click(getByText('Restore'));
    const keywordInput = document.getElementById('confirm-keyword-input');
    await fireEvent.input(keywordInput, { target: { value: 'RESTORE' } });
    await fireEvent.click(getByText('Restore', { selector: '.btn-danger' }));

    await waitFor(() => expect(onNotify).toHaveBeenCalledWith(expect.stringContaining('Access denied'), 'error'));
  });

  it('calls `onBack` on Back', async () => {
    getBackups.mockResolvedValue([]);
    const onBack = vi.fn();
    const { getByText } = render(BackupManager, { props: { onBack } });
    await waitFor(() => expect(getByText('No backup available yet.')).toBeInTheDocument());
    await fireEvent.click(getByText('Back'));
    expect(onBack).toHaveBeenCalled();
  });
});
