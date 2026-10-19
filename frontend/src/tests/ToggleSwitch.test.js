import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import ToggleSwitch from '../lib/components/ToggleSwitch.svelte';

describe('ToggleSwitch', () => {
  it('shows the label', () => {
    const { getByText } = render(ToggleSwitch, { props: { label: 'Mock service-a', checked: false } });
    expect(getByText('Mock service-a')).toBeInTheDocument();
  });

  it('shows OFF when unchecked', () => {
    const { getByText } = render(ToggleSwitch, { props: { label: 'Test', checked: false } });
    expect(getByText('OFF')).toBeInTheDocument();
  });

  it('shows ON when checked', () => {
    const { getByText } = render(ToggleSwitch, { props: { label: 'Test', checked: true } });
    expect(getByText('ON')).toBeInTheDocument();
  });

  it('has the switch role with `aria-checked`', () => {
    const { getByRole } = render(ToggleSwitch, { props: { label: 'Test', checked: true } });
    const btn = getByRole('switch');
    expect(btn).toHaveAttribute('aria-checked', 'true');
  });

  it('calls `onchange` with the new state on click', async () => {
    const onchange = vi.fn();
    const { getByRole } = render(ToggleSwitch, { props: { label: 'Test', checked: false, onchange } });
    await fireEvent.click(getByRole('switch'));
    expect(onchange).toHaveBeenCalledWith(true);
  });

  it('does not call `onchange` when disabled', async () => {
    const onchange = vi.fn();
    const { getByRole } = render(ToggleSwitch, { props: { label: 'Test', checked: false, disabled: true, onchange } });
    await fireEvent.click(getByRole('switch'));
    expect(onchange).not.toHaveBeenCalled();
  });
});
