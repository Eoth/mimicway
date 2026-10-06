<script>
  import { getGroups, createGroup, deleteGroup, updateGroupMembers, updateService } from '../api.js';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import RemovableList from './RemovableList.svelte';
  import FormField from './FormField.svelte';
  import { t, tCount } from '../i18n.svelte.js';

  let {
    services = [],
    authEnabled = false,
    onNotify = () => {},
    onBack = () => {},
    onServiceUpdate = () => {},
    onGroupsChange = () => {},
  } = $props();

  let groups = $state([]);
  let loading = $state(true);
  let showForm = $state(false);
  let newGroupName = $state('');
  let editingGroup = $state(null);
  let newMember = $state('');
  let newAdmin = $state('');
  let formError = $state('');
  let groupPendingDelete = $state(null);

  let servicesOfGroup = $derived((groupName) => services.filter((s) => s.group_name === groupName));
  let ungroupedServices = $derived(services.filter((s) => !s.group_name));

  function setGroups(newGroups) {
    groups = newGroups;
    onGroupsChange(newGroups);
  }

  async function loadGroups() {
    try {
      const loaded = await getGroups();
      setGroups(loaded);
    } catch (e) {
      onNotify(t('Error while loading the groups: {0}', e.message), 'error');
    } finally {
      loading = false;
    }
  }

  async function handleCreateGroup(e) {
    e.preventDefault();
    formError = '';
    const name = newGroupName.trim();
    if (!name) {
      formError = t('The group name is required.');
      return;
    }

    try {
      const created = await createGroup({ name, code: '', admins: [], members: [] });
      setGroups([...groups, created]);
      newGroupName = '';
      showForm = false;
      onNotify(t('Group "{0}" created', name), 'success');
    } catch (e) {
      formError = e.message;
    }
  }

  async function handleDeleteGroup(name) {
    groupPendingDelete = null;
    try {
      await deleteGroup(name);
      setGroups(groups.filter((g) => g.name !== name));
      if (editingGroup === name) editingGroup = null;
      for (const svc of services.filter((s) => s.group_name === name)) {
        onServiceUpdate({ ...svc, group_name: null }, svc.group_name);
      }
      onNotify(t('Group "{0}" deleted, its services are now ungrouped', name), 'success');
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }

  function startEdit(name) {
    editingGroup = editingGroup === name ? null : name;
    newMember = '';
    newAdmin = '';
  }

  async function assignServiceToGroup(serviceName, groupName) {
    const svc = services.find((s) => s.name === serviceName);
    if (!svc) return;
    try {
      const updated = await updateService(serviceName, svc.group_name, { ...svc, group_name: groupName || null });
      onServiceUpdate(updated, svc.group_name);
      onNotify(t('Service "{0}" added to the group "{1}"', serviceName, groupName), 'success');
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }

  async function removeServiceFromGroup(serviceName) {
    const svc = services.find((s) => s.name === serviceName);
    if (!svc) return;
    try {
      const payload = { ...svc };
      delete payload.group_name;
      const updated = await updateService(serviceName, svc.group_name, payload);
      onServiceUpdate(updated, svc.group_name);
      onNotify(t('Service "{0}" removed from the group', serviceName), 'success');
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }

  async function addMember(groupName) {
    const username = newMember.trim();
    if (!username) return;
    const group = groups.find((g) => g.name === groupName);
    if (!group) return;
    if (group.members.includes(username) || group.admins.includes(username)) {
      onNotify(t('"{0}" is already in the group', username), 'error');
      return;
    }
    try {
      const updated = await updateGroupMembers(groupName, {
        admins: group.admins,
        members: [...group.members, username],
      });
      setGroups(groups.map((g) => (g.name === groupName ? updated : g)));
      newMember = '';
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }

  async function addAdmin(groupName) {
    const username = newAdmin.trim();
    if (!username) return;
    const group = groups.find((g) => g.name === groupName);
    if (!group) return;
    if (group.admins.includes(username)) {
      onNotify(t('"{0}" is already an admin', username), 'error');
      return;
    }
    try {
      const members = group.members.filter((m) => m !== username);
      const updated = await updateGroupMembers(groupName, {
        admins: [...group.admins, username],
        members,
      });
      setGroups(groups.map((g) => (g.name === groupName ? updated : g)));
      newAdmin = '';
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }

  async function removePerson(groupName, username, role) {
    const group = groups.find((g) => g.name === groupName);
    if (!group) return;
    try {
      const admins = role === 'admin' ? group.admins.filter((a) => a !== username) : group.admins;
      const members = role === 'member' ? group.members.filter((m) => m !== username) : group.members;
      const updated = await updateGroupMembers(groupName, { admins, members });
      setGroups(groups.map((g) => (g.name === groupName ? updated : g)));
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }

  $effect(() => {
    loadGroups();
  });
</script>

<div class="group-manager">
  <div class="list-header">
    <h2>{t('Service groups')}</h2>
    <div class="header-actions">
      <button
        type="button"
        class="btn btn-sm {showForm ? 'btn-secondary' : 'btn-primary'}"
        onclick={() => {
          showForm = !showForm;
          formError = '';
        }}
        data-testid="group-manager-new-group-button"
      >
        {showForm ? t('Cancel') : t('+ New group')}
      </button>
      <button type="button" class="btn btn-outline btn-sm" onclick={onBack} data-testid="group-manager-back-button"
        >{t('Back')}</button
      >
    </div>
  </div>

  {#if showForm}
    <form class="group-create-form" onsubmit={handleCreateGroup} data-testid="group-manager-create-form">
      <FormField
        id="new-group-name"
        label={t('Group name')}
        required
        error={formError}
        hint={t('A 5-character URL code is generated from the name.')}
      >
        {#snippet children({ id, describedBy, invalid })}
          <input
            type="text"
            {id}
            aria-describedby={describedBy}
            aria-invalid={invalid}
            bind:value={newGroupName}
            placeholder={t('e.g. Internal APIs')}
            required
            data-testid="group-manager-name-input"
          />
        {/snippet}
      </FormField>
      <div class="create-actions">
        <button type="submit" class="btn btn-primary btn-sm" data-testid="group-manager-create-submit-button"
          >{t('Create')}</button
        >
      </div>
    </form>
  {/if}

  {#if loading}
    <p class="loading-text">{t('Loading the groups...')}</p>
  {:else if groups.length === 0}
    <p class="empty-text">{t('No group. Create one to organize your services by domain.')}</p>
  {:else}
    <div class="group-list">
      {#each groups as group}
        {@const groupServices = servicesOfGroup(group.name)}
        <div class="group-card" data-testid="group-manager-card-{group.name}">
          <div class="group-header-row">
            <h3>{group.name}</h3>
            <span class="group-code-badge">/{group.code}</span>
            <span class="group-count">{tCount(groupServices.length, '{0} service', '{0} services')}</span>
            <div class="group-actions">
              <button
                type="button"
                class="btn btn-outline btn-sm"
                onclick={() => startEdit(group.name)}
                data-testid="group-manager-manage-button-{group.name}"
              >
                {editingGroup === group.name ? t('Close') : t('Manage')}
              </button>
              <button
                type="button"
                class="btn btn-danger-outline btn-sm"
                onclick={() => (groupPendingDelete = group.name)}
                data-testid="group-manager-delete-button-{group.name}"
              >
                {t('Delete')}
              </button>
            </div>
          </div>

          {#if groupServices.length > 0}
            <ul class="service-chips">
              {#each groupServices as svc}
                <li>
                  <span class="service-chip">
                    {svc.name}
                    <button
                      type="button"
                      class="chip-remove"
                      onclick={() => removeServiceFromGroup(svc.name)}
                      title={t('Remove from the group')}
                      aria-label={t('Remove {0} from the group', svc.name)}
                      data-testid="group-manager-remove-service-button-{group.name}-{svc.name}">x</button
                    >
                  </span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="empty-hint">{t('No service in this group. Use the menu below to add some.')}</p>
          {/if}

          {#if editingGroup === group.name}
            <div class="group-edit">
              <div class="edit-section">
                <h4>{t('Add a service')}</h4>
                {#if ungroupedServices.length === 0}
                  <p class="empty-hint">{t('Every service is already in a group.')}</p>
                {:else}
                  <div class="service-assign-list">
                    {#each ungroupedServices as svc}
                      <button
                        type="button"
                        class="btn btn-outline btn-sm"
                        onclick={() => assignServiceToGroup(svc.name, group.name)}
                        data-testid="group-manager-assign-service-button-{group.name}-{svc.name}"
                      >
                        + {svc.name}
                      </button>
                    {/each}
                  </div>
                {/if}
              </div>

              {#if authEnabled}
                <div class="edit-section">
                  <h4>{t('Administrators')}</h4>
                  <RemovableList
                    items={group.admins}
                    onRemove={(admin) => removePerson(group.name, admin, 'admin')}
                    emptyText={t('No administrator')}
                  />
                  <div class="inline-form">
                    <input
                      type="text"
                      bind:value={newAdmin}
                      placeholder={t('Add an admin')}
                      data-testid="group-manager-new-admin-input-{group.name}"
                    />
                    <button
                      type="button"
                      class="btn btn-outline btn-sm"
                      onclick={() => addAdmin(group.name)}
                      data-testid="group-manager-add-admin-button-{group.name}">+</button
                    >
                  </div>
                </div>

                <div class="edit-section">
                  <h4>{t('Members')}</h4>
                  <RemovableList
                    items={group.members}
                    onRemove={(member) => removePerson(group.name, member, 'member')}
                    emptyText={t('No member')}
                  />
                  <div class="inline-form">
                    <input
                      type="text"
                      bind:value={newMember}
                      placeholder={t('Add a member')}
                      data-testid="group-manager-new-member-input-{group.name}"
                    />
                    <button
                      type="button"
                      class="btn btn-outline btn-sm"
                      onclick={() => addMember(group.name)}
                      data-testid="group-manager-add-member-button-{group.name}">+</button
                    >
                  </div>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <ConfirmDialog
    open={groupPendingDelete !== null}
    title={t('Delete the group')}
    message={groupPendingDelete
      ? t('Delete the group "{0}"? Its services will no longer belong to any group.', groupPendingDelete)
      : ''}
    confirmLabel={t('Yes, delete')}
    onConfirm={() => handleDeleteGroup(groupPendingDelete)}
    onCancel={() => (groupPendingDelete = null)}
  />
</div>

<style>
  .group-manager {
    max-width: 60rem;
  }
  .list-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-4);
  }
  .list-header h2 {
    margin: 0;
  }
  .header-actions {
    display: flex;
    gap: var(--space-2);
  }

  .group-create-form {
    margin-bottom: var(--space-4);
    max-width: 24rem;
  }
  .create-actions {
    display: flex;
    gap: var(--space-3);
  }
  .inline-form {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .inline-form input {
    flex: 1;
    padding: var(--space-1-5) var(--space-3);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-m);
    background: var(--color-bg);
    color: var(--color-text);
  }

  .group-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .group-card {
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-4) var(--space-5);
  }

  .group-header-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .group-header-row h3 {
    margin: 0;
    font-size: var(--text-l);
  }
  .group-code-badge {
    font-family: var(--font-code);
    font-size: var(--text-s);
    font-weight: var(--weight-heavy);
    color: var(--color-primary);
    background: var(--color-selected);
    padding: var(--space-0-5) var(--space-1-5);
    border-radius: var(--radius-s);
  }
  .group-count {
    color: var(--color-text-muted);
    font-size: var(--text-s);
  }
  .group-actions {
    margin-left: auto;
    display: flex;
    gap: var(--space-1-5);
  }

  .service-chips {
    list-style: none;
    padding: 0;
    margin: var(--space-3) 0 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1-5);
  }
  .service-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1-5);
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-pill);
    background: var(--color-selected);
    color: var(--color-primary);
    font-size: var(--text-s);
    font-weight: var(--weight-strong);
  }
  .chip-remove {
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    font-weight: var(--weight-heavy);
    font-size: var(--text-xs);
    padding: 0;
    opacity: 0.7;
  }
  .chip-remove:hover {
    opacity: 1;
  }

  .empty-hint {
    color: var(--color-text-muted);
    font-size: var(--text-s);
    margin: var(--space-2) 0;
    font-style: italic;
  }

  .group-edit {
    margin-top: var(--space-4);
    padding-top: var(--space-4);
    border-top: var(--line-thin) solid var(--color-border);
    display: flex;
    gap: var(--space-8);
    flex-wrap: wrap;
  }
  .edit-section {
    flex: 1;
    min-width: 14rem;
  }
  .edit-section h4 {
    margin: 0 0 var(--space-2);
    font-size: var(--text-m);
    color: var(--color-text-muted);
  }

  .service-assign-list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1-5);
  }

  .loading-text,
  .empty-text {
    color: var(--color-text-muted);
    font-size: var(--text-m);
    text-align: center;
    padding: var(--space-4);
  }
</style>
