import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import ConfirmDialog from '../lib/components/ConfirmDialog.svelte';

describe('ConfirmDialog', () => {
  it('ne rend rien quand open est false', () => {
    const { queryByRole } = render(ConfirmDialog, { props: { open: false, title: 'Test' } });
    expect(queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('affiche le titre et le message quand open est true', () => {
    const { getByRole, getByText } = render(ConfirmDialog, {
      props: { open: true, title: 'Delete the service', message: 'This cannot be undone.' },
    });
    expect(getByRole('dialog')).toBeInTheDocument();
    expect(getByText('Delete the service')).toBeInTheDocument();
    expect(getByText('This cannot be undone.')).toBeInTheDocument();
  });

  it('appelle onConfirm au clic sur le bouton de confirmation', async () => {
    const onConfirm = vi.fn();
    const { getByText } = render(ConfirmDialog, {
      props: { open: true, title: 'Test', confirmLabel: 'Yes, delete', onConfirm },
    });
    await fireEvent.click(getByText('Yes, delete'));
    expect(onConfirm).toHaveBeenCalled();
  });

  it('appelle onCancel au clic sur Annuler', async () => {
    const onCancel = vi.fn();
    const { getByText } = render(ConfirmDialog, {
      props: { open: true, title: 'Test', onCancel },
    });
    await fireEvent.click(getByText('Cancel'));
    expect(onCancel).toHaveBeenCalled();
  });

  it('appelle onCancel sur la touche Escape', async () => {
    const onCancel = vi.fn();
    const { getByRole } = render(ConfirmDialog, {
      props: { open: true, title: 'Test', onCancel },
    });
    await fireEvent.keyDown(getByRole('dialog'), { key: 'Escape' });
    expect(onCancel).toHaveBeenCalled();
  });

  it("desactive le bouton de confirmation tant que le mot-cle n'est pas saisi exactement", async () => {
    const onConfirm = vi.fn();
    const { getByText, getByLabelText } = render(ConfirmDialog, {
      props: { open: true, title: 'Reset', confirmLabel: 'Confirm', confirmKeyword: 'RESET', onConfirm },
    });

    const confirmBtn = getByText('Confirm');
    expect(confirmBtn).toBeDisabled();

    const input = getByLabelText(/RESET/);
    await fireEvent.input(input, { target: { value: 'wrong' } });
    expect(confirmBtn).toBeDisabled();

    await fireEvent.input(input, { target: { value: 'RESET' } });
    await waitFor(() => expect(confirmBtn).not.toBeDisabled());

    await fireEvent.click(confirmBtn);
    expect(onConfirm).toHaveBeenCalled();
  });

  it('remet le champ mot-cle a vide a chaque ouverture', async () => {
    const { getByLabelText, rerender } = render(ConfirmDialog, {
      props: { open: true, title: 'Reset', confirmKeyword: 'RESET' },
    });
    const input = getByLabelText(/RESET/);
    await fireEvent.input(input, { target: { value: 'RESET' } });
    expect(input.value).toBe('RESET');

    await rerender({ open: false, title: 'Reset', confirmKeyword: 'RESET' });
    await rerender({ open: true, title: 'Reset', confirmKeyword: 'RESET' });

    expect(getByLabelText(/RESET/).value).toBe('');
  });
});
