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

  it("affiche un etat vide quand aucune sauvegarde n'existe", async () => {
    getBackups.mockResolvedValue([]);
    const { getByText } = render(BackupManager);
    await waitFor(() => expect(getByText('No backup available yet.')).toBeInTheDocument());
  });

  it('affiche la liste des sauvegardes avec nom, taille et badge protege', async () => {
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

  it("ouvre la confirmation au clic sur Restaurer sans appeler l'API tout de suite", async () => {
    getBackups.mockResolvedValue([
      { filename: 'mock-config-1-000001.yaml', protected: false, size_bytes: 100, created_at_ms: 1690000000000 },
    ]);
    const { getByText, getByRole } = render(BackupManager);

    await waitFor(() => expect(getByText('mock-config-1-000001.yaml')).toBeInTheDocument());
    await fireEvent.click(getByText('Restore'));

    expect(getByRole('dialog')).toBeInTheDocument();
    expect(restoreBackup).not.toHaveBeenCalled();
  });

  it('appelle restoreBackup et recharge la liste apres confirmation', async () => {
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

  it("annule sans appeler l'API", async () => {
    getBackups.mockResolvedValue([
      { filename: 'mock-config-1-000001.yaml', protected: false, size_bytes: 100, created_at_ms: 1690000000000 },
    ]);
    const { getByText } = render(BackupManager);

    await waitFor(() => expect(getByText('mock-config-1-000001.yaml')).toBeInTheDocument());
    await fireEvent.click(getByText('Restore'));
    await fireEvent.click(getByText('Cancel'));

    expect(restoreBackup).not.toHaveBeenCalled();
  });

  it('notifie une erreur quand le backend refuse (403, droits admin requis)', async () => {
    getBackups.mockRejectedValue(new Error('Access denied'));
    const onNotify = vi.fn();
    render(BackupManager, { props: { onNotify } });

    await waitFor(() => expect(onNotify).toHaveBeenCalledWith(expect.stringContaining('Access denied'), 'error'));
  });

  it('notifie une erreur si la restauration est refusee (403)', async () => {
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

  it('appelle onBack au clic sur Retour', async () => {
    getBackups.mockResolvedValue([]);
    const onBack = vi.fn();
    const { getByText } = render(BackupManager, { props: { onBack } });
    await waitFor(() => expect(getByText('No backup available yet.')).toBeInTheDocument());
    await fireEvent.click(getByText('Back'));
    expect(onBack).toHaveBeenCalled();
  });
});
