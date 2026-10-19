import { render } from '@testing-library/svelte';
import { describe, it, expect } from 'vitest';
import Notification from '../lib/components/Notification.svelte';

describe('Notification', () => {
  it('shows nothing when `visible` is false', () => {
    const { queryByRole } = render(Notification, { props: { message: 'test', visible: false } });
    expect(queryByRole('alert')).not.toBeInTheDocument();
  });

  it('shows the message when `visible` is true', () => {
    const { getByText } = render(Notification, { props: { message: 'Saved', type: 'success', visible: true } });
    expect(getByText('Saved')).toBeInTheDocument();
  });

  it('has the alert role, so screen readers announce it', () => {
    const { getByRole } = render(Notification, { props: { message: 'Failed', type: 'error', visible: true } });
    expect(getByRole('alert')).toBeInTheDocument();
  });
});
