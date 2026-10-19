import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ServiceList from '../lib/components/ServiceList.svelte';
import { resetGroupExpansionState } from '../lib/group-expansion-state.svelte.js';

const mockServices = [
  {
    name: 'svc-users',
    listen_path: '/users/*',
    real_target_url: 'http://users:80',
    is_mocked: true,
    group_name: null,
    rules: [{ name: 'r1' }],
  },
  {
    name: 'svc-orders',
    listen_path: '/orders/*',
    real_target_url: 'http://orders:80',
    is_mocked: false,
    group_name: null,
    rules: [],
  },
  {
    name: 'insee-api',
    listen_path: '/v4/api/insee/*',
    real_target_url: 'http://insee:80',
    is_mocked: true,
    group_name: 'team-a',
    rules: [{ name: 'siret' }],
  },
];

const searchPlaceholder = 'Search by name, path, URL or group...';

describe('ServiceList', () => {
  beforeEach(() => {
    // The expansion state lives in a shared store that outlives the component (group-expansion-state.svelte.js):
    // reset it so that each test starts with every group folded.
    resetGroupExpansionState();
  });

  it('shows an empty state when there is no service', () => {
    const { getByText } = render(ServiceList, { props: { services: [] } });
    expect(getByText('No service configured')).toBeInTheDocument();
  });

  it('shows the groups folded by default', () => {
    const { getByText, queryByText } = render(ServiceList, { props: { services: mockServices } });
    expect(getByText('team-a')).toBeInTheDocument();
    expect(getByText('No group')).toBeInTheDocument();
    expect(queryByText('svc-users')).not.toBeInTheDocument();
  });

  it('shows the search bar when there are services', () => {
    const { getByPlaceholderText } = render(ServiceList, { props: { services: mockServices } });
    expect(getByPlaceholderText(searchPlaceholder)).toBeInTheDocument();
  });

  it('filters by service name', async () => {
    const { getByPlaceholderText, queryByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: 'insee' } });
    expect(queryByText('insee-api')).toBeInTheDocument();
    expect(queryByText('svc-users')).not.toBeInTheDocument();
    expect(queryByText('svc-orders')).not.toBeInTheDocument();
  });

  it('filters by listen path', async () => {
    const { getByPlaceholderText, queryByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: '/orders' } });
    expect(queryByText('svc-orders')).toBeInTheDocument();
    expect(queryByText('svc-users')).not.toBeInTheDocument();
  });

  it('shows a message when the search matches nothing', async () => {
    const { getByPlaceholderText, getByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: 'zzzzz' } });
    expect(getByText('No service matches “zzzzz”')).toBeInTheDocument();
  });

  it('shows the result count during a search', async () => {
    const { getByPlaceholderText, getByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: 'svc' } });
    expect(getByText('2 / 3 services')).toBeInTheDocument();
  });

  it('filters by group name', async () => {
    const { getByPlaceholderText, queryByText } = render(ServiceList, { props: { services: mockServices } });
    const search = getByPlaceholderText(searchPlaceholder);
    await fireEvent.input(search, { target: { value: 'team-a' } });
    expect(queryByText('insee-api')).toBeInTheDocument();
    expect(queryByText('svc-users')).not.toBeInTheDocument();
  });

  it('a group unfolded by hand stays unfolded after the component is unmounted and mounted again', async () => {
    const first = render(ServiceList, { props: { services: mockServices } });
    await fireEvent.click(first.getByText('team-a'));
    expect(first.queryByText('insee-api')).toBeInTheDocument();
    first.unmount();

    // As when the user comes back from editing a service: `ServiceList` is mounted from scratch, and only the shared
    // store still holds the expansion state.
    const second = render(ServiceList, { props: { services: mockServices } });
    expect(second.queryByText('insee-api')).toBeInTheDocument();
  });

  it('selecting a service found by a search opens its group for the return to the list', async () => {
    const onSelect = vi.fn();
    const first = render(ServiceList, { props: { services: mockServices, onSelect } });
    // A search shows the service without opening its group: only the selection can leave the group open.
    await fireEvent.input(first.getByPlaceholderText(searchPlaceholder), { target: { value: 'insee' } });
    const configureBtn = first.getByRole('button', { name: 'Configure the service insee-api' });
    await fireEvent.click(configureBtn);
    expect(onSelect).toHaveBeenCalledWith('insee-api', 'team-a');
    first.unmount();

    const second = render(ServiceList, { props: { services: mockServices } });
    expect(second.queryByText('insee-api')).toBeInTheDocument();
  });

  it('a group folded by hand stays folded after the component is unmounted and mounted again', async () => {
    const first = render(ServiceList, { props: { services: mockServices } });
    await fireEvent.click(first.getByText('team-a'));
    await fireEvent.click(first.getByText('team-a'));
    expect(first.queryByText('insee-api')).not.toBeInTheDocument();
    first.unmount();

    const second = render(ServiceList, { props: { services: mockServices } });
    expect(second.queryByText('insee-api')).not.toBeInTheDocument();
  });
});
