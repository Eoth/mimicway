import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import ServiceCard from '../lib/components/ServiceCard.svelte';

const mockService = {
  name: 'svc-users',
  listen_path: '/users/*',
  real_target_url: 'http://users:80',
  is_mocked: true,
  rules: [{ name: 'r1' }, { name: 'r2' }],
};

describe('ServiceCard', () => {
  it('shows the service name', () => {
    const { getByText } = render(ServiceCard, { props: { service: mockService } });
    expect(getByText('svc-users')).toBeInTheDocument();
  });

  it('shows the URL prefixed with the service name', () => {
    const { getByText } = render(ServiceCard, { props: { service: mockService } });
    expect(getByText('/svc-users/users/*')).toBeInTheDocument();
  });

  it('shows the target URL', () => {
    const { getByText } = render(ServiceCard, { props: { service: mockService } });
    expect(getByText('http://users:80')).toBeInTheDocument();
  });

  it('shows the number of rules', () => {
    const { getByText } = render(ServiceCard, { props: { service: mockService } });
    expect(getByText('2')).toBeInTheDocument();
  });

  it('has a Configure button', () => {
    const { getByText } = render(ServiceCard, { props: { service: mockService } });
    expect(getByText('Configure')).toBeInTheDocument();
  });

  it('calls `onSelect` with the service name and group on Configure', async () => {
    const onSelect = vi.fn();
    const svc = { ...mockService, group_name: 'team-a' };
    const { getByText } = render(ServiceCard, { props: { service: svc, onSelect } });
    await fireEvent.click(getByText('Configure'));
    expect(onSelect).toHaveBeenCalledWith('svc-users', 'team-a');
  });

  it('shows the MOCK badge when `is_mocked` is true', () => {
    const { getByText } = render(ServiceCard, { props: { service: mockService } });
    expect(getByText('MOCK')).toBeInTheDocument();
  });

  it('shows the PROXY badge when `is_mocked` is false', () => {
    const svc = { ...mockService, is_mocked: false };
    const { getByText } = render(ServiceCard, { props: { service: svc } });
    expect(getByText('PROXY')).toBeInTheDocument();
  });
});
