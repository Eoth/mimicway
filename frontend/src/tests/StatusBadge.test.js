import { render } from '@testing-library/svelte';
import { describe, it, expect } from 'vitest';
import StatusBadge from '../lib/components/StatusBadge.svelte';

describe('StatusBadge', () => {
  it('shows MOCK when active', () => {
    const { getByText } = render(StatusBadge, { props: { active: true } });
    expect(getByText('MOCK')).toBeInTheDocument();
  });

  it('shows PROXY when inactive', () => {
    const { getByText } = render(StatusBadge, { props: { active: false } });
    expect(getByText('PROXY')).toBeInTheDocument();
  });

  it('has the status role', () => {
    const { getByRole } = render(StatusBadge, { props: { active: true } });
    expect(getByRole('status')).toBeInTheDocument();
  });
});
