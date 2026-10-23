import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import GroupManager from '../lib/components/GroupManager.svelte';
import { getGroups, createGroup, updateService } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  getGroups: vi.fn(),
  createGroup: vi.fn(),
  deleteGroup: vi.fn(),
  updateGroupMembers: vi.fn(),
  updateService: vi.fn(),
}));

describe('GroupManager - creation form', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getGroups.mockResolvedValue([]);
  });

  it('shows a name field only, with no field for the code', async () => {
    const { getByText, container } = render(GroupManager);
    await waitFor(() =>
      expect(getByText('No group. Create one to organize your services by domain.')).toBeInTheDocument(),
    );

    await fireEvent.click(getByText('+ New group'));

    const form = container.querySelector('.group-create-form');
    expect(form).toBeInTheDocument();
    const inputs = form.querySelectorAll('input');
    expect(inputs.length).toBe(1);
    expect(inputs[0].id).toBe('new-group-name');
  });

  it('creates a group from its name alone, sending an empty code for the server to generate', async () => {
    createGroup.mockResolvedValue({ name: 'Internal APIs', code: 'ab3f9', admins: [], members: [] });
    const onNotify = vi.fn();
    const { getByText, container } = render(GroupManager, { props: { onNotify } });
    await waitFor(() =>
      expect(getByText('No group. Create one to organize your services by domain.')).toBeInTheDocument(),
    );

    await fireEvent.click(getByText('+ New group'));
    const nameInput = container.querySelector('#new-group-name');
    nameInput.value = 'Internal APIs';
    await fireEvent.input(nameInput);
    await fireEvent.submit(container.querySelector('form'));

    await waitFor(() =>
      expect(createGroup).toHaveBeenCalledWith({
        name: 'Internal APIs',
        code: '',
        admins: [],
        members: [],
      }),
    );
    expect(onNotify).toHaveBeenCalledWith('Group "Internal APIs" created', 'success');
  });

  it('sends a name with accents and spaces as typed', async () => {
    createGroup.mockResolvedValue({ name: 'Équipe Café', code: 'k2m81', admins: [], members: [] });
    const { getByText, container } = render(GroupManager);
    await waitFor(() =>
      expect(getByText('No group. Create one to organize your services by domain.')).toBeInTheDocument(),
    );

    await fireEvent.click(getByText('+ New group'));
    const nameInput = container.querySelector('#new-group-name');
    nameInput.value = 'Équipe Café';
    await fireEvent.input(nameInput);
    await fireEvent.submit(container.querySelector('form'));

    await waitFor(() =>
      expect(createGroup).toHaveBeenCalledWith({
        name: 'Équipe Café',
        code: '',
        admins: [],
        members: [],
      }),
    );
  });

  it('shows the server error, such as a name already taken, on the name field', async () => {
    createGroup.mockRejectedValue(new Error('A group named "Ops" already exists.'));
    const { getByText, container } = render(GroupManager);
    await waitFor(() =>
      expect(getByText('No group. Create one to organize your services by domain.')).toBeInTheDocument(),
    );

    await fireEvent.click(getByText('+ New group'));
    const nameInput = container.querySelector('#new-group-name');
    nameInput.value = 'Ops';
    await fireEvent.input(nameInput);
    await fireEvent.submit(container.querySelector('form'));

    await waitFor(() => expect(getByText('A group named "Ops" already exists.')).toBeInTheDocument());
    expect(nameInput.getAttribute('aria-invalid')).toBe('true');
  });

  it('refuses an empty name without calling the API', async () => {
    const { getByText, container } = render(GroupManager);
    await waitFor(() =>
      expect(getByText('No group. Create one to organize your services by domain.')).toBeInTheDocument(),
    );

    await fireEvent.click(getByText('+ New group'));
    await fireEvent.submit(container.querySelector('form'));

    await waitFor(() => expect(getByText('The group name is required.')).toBeInTheDocument());
    expect(createGroup).not.toHaveBeenCalled();
  });
});

describe('GroupManager - services with the same name in different groups', () => {
  const service = (name, group_name) => ({ name, group_name, rules: [] });
  const services = [service('users', 'team-a'), service('users', 'team-b'), service('users', null)];

  beforeEach(() => {
    vi.clearAllMocks();
    getGroups.mockResolvedValue([
      { name: 'team-a', code: 'tmaaa', admins: [], members: [] },
      { name: 'team-b', code: 'tmbbb', admins: [], members: [] },
    ]);
    updateService.mockImplementation((_name, _group, payload) => Promise.resolve(payload));
  });

  it('removes from a group the service of that group, not a namesake of another group', async () => {
    const { findByTestId } = render(GroupManager, { props: { services } });
    await fireEvent.click(await findByTestId('group-manager-remove-service-button-team-b-users'));

    await waitFor(() => expect(updateService).toHaveBeenCalledTimes(1));
    const [name, group, payload] = updateService.mock.calls[0];
    expect([name, group]).toEqual(['users', 'team-b']);
    expect(payload.group_name ?? null).toBeNull();
  });

  it('adds to a group the ungrouped service clicked, not a namesake that has a group', async () => {
    const { findByTestId } = render(GroupManager, { props: { services } });
    await fireEvent.click(await findByTestId('group-manager-manage-button-team-b'));
    await fireEvent.click(await findByTestId('group-manager-assign-service-button-team-b-users'));

    await waitFor(() => expect(updateService).toHaveBeenCalledTimes(1));
    const [name, group, payload] = updateService.mock.calls[0];
    expect([name, group]).toEqual(['users', null]);
    expect(payload.group_name).toBe('team-b');
  });
});
