import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import ConfirmDialog from '../lib/components/ConfirmDialog.svelte';

describe('ConfirmDialog', () => {
  it('renders nothing when `open` is false', () => {
    const { queryByRole } = render(ConfirmDialog, { props: { open: false, title: 'Test' } });
    expect(queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows the title and the message when `open` is true', () => {
    const { getByRole, getByText } = render(ConfirmDialog, {
      props: { open: true, title: 'Delete the service', message: 'This cannot be undone.' },
    });
    expect(getByRole('dialog')).toBeInTheDocument();
    expect(getByText('Delete the service')).toBeInTheDocument();
    expect(getByText('This cannot be undone.')).toBeInTheDocument();
  });

  it('calls `onConfirm` on the confirm button', async () => {
    const onConfirm = vi.fn();
    const { getByText } = render(ConfirmDialog, {
      props: { open: true, title: 'Test', confirmLabel: 'Yes, delete', onConfirm },
    });
    await fireEvent.click(getByText('Yes, delete'));
    expect(onConfirm).toHaveBeenCalled();
  });

  it('calls `onCancel` on Cancel', async () => {
    const onCancel = vi.fn();
    const { getByText } = render(ConfirmDialog, {
      props: { open: true, title: 'Test', onCancel },
    });
    await fireEvent.click(getByText('Cancel'));
    expect(onCancel).toHaveBeenCalled();
  });

  it('calls `onCancel` on Escape', async () => {
    const onCancel = vi.fn();
    const { getByRole } = render(ConfirmDialog, {
      props: { open: true, title: 'Test', onCancel },
    });
    await fireEvent.keyDown(getByRole('dialog'), { key: 'Escape' });
    expect(onCancel).toHaveBeenCalled();
  });

  it('keeps the confirm button disabled until the keyword is typed exactly', async () => {
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

  it('clears the keyword field each time it opens', async () => {
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
