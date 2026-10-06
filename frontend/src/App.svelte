<script>
  import {
    getServices,
    getConfig,
    putConfig,
    toggleService,
    createService,
    updateService,
    resetConfig,
    getAuthStatus,
    validateToken,
    getGroups,
    createGroup,
    getMessagingStatus,
    getTcpStatus,
  } from './lib/api.js';
  import { auth, isLoggedIn, setAuth, logout, restoreAuth } from './lib/auth.svelte.js';
  import ServiceList from './lib/components/ServiceList.svelte';
  import ServiceDetail from './lib/components/ServiceDetail.svelte';
  import ServiceForm from './lib/components/ServiceForm.svelte';
  import Notification from './lib/components/Notification.svelte';
  import RequestLog from './lib/components/RequestLog.svelte';
  import MessagingLog from './lib/components/MessagingLog.svelte';
  import TcpServiceManager from './lib/components/TcpServiceManager.svelte';
  import LoginForm from './lib/components/LoginForm.svelte';
  import GroupManager from './lib/components/GroupManager.svelte';
  import BackupManager from './lib/components/BackupManager.svelte';
  import ConfirmDialog from './lib/components/ConfirmDialog.svelte';
  import { t, getLocale, setLocale, LOCALES } from './lib/i18n.svelte.js';
  import { applyTheme, initialTheme, saveTheme } from './lib/theme.js';

  let services = $state([]);
  let resetPending = $state(false);
  let groups = $state([]);
  // False on a binary built without the "messaging-kafka" feature: the Kafka messages button stays hidden rather than
  // leading to a view that fails (404).
  let messagingAvailable = $state(false);
  // The same guard, for the "tcp-mock" feature.
  let tcpAvailable = $state(false);
  let notification = $state({ message: '', type: 'info', visible: false });
  let selectedService = $state(null);
  // A name alone does not identify a service (two groups may each hold one of that name): the group is kept next to
  // the selected name, so that currentService, handleServiceUpdate and handleServiceDelete find the right service.
  let selectedServiceGroup = $state(null);
  let view = $state('list');
  let loading = $state(true);
  let darkMode = $state(initialTheme() === 'dark');

  $effect(() => {
    applyTheme(darkMode ? 'dark' : 'light');
    saveTheme(darkMode ? 'dark' : 'light');
  });

  const demoService = {
    name: 'users-api',
    listen_path: '/users/{id}',
    real_target_url: 'https://jsonplaceholder.typicode.com',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [
      {
        name: 'get-user-mock',
        method: 'GET',
        sub_path: null,
        action: 'mock',
        pre_script: null,
        script:
          'let role = if random_int(1,5) <= 4 { "user" } else { "admin" };\n#{ role: role, since: date_past(random_int(1, 1825)) }',
        post_script: null,
        conditions: { all_of: [], any_of: [] },
        response: {
          status: 200,
          headers: [{ name: 'Content-Type', value: 'application/json' }],
          body: [
            {
              type: 'Template',
              template:
                '{"id":{{path.id}},"name":"{{fake.FirstName}} {{fake.LastName}}","email":"{{fake.Email}}","phone":"{{fake.PhoneNumberFR}}","company":"{{fake.CompanyName}}","role":"{{script.role}}","member_since":"{{script.since}}","address":{"street":"{{fake.StreetName}}","city":"{{fake.CityFR}}","zipcode":"{{fake.PostcodeFR}}"},"meta":{"request_id":"{{uuid}}","timestamp":{{now_ms}},"seq":{{seq}}}}',
            },
          ],
          chaos: null,
        },
      },
      {
        name: 'get-user-proxy',
        method: 'GET',
        sub_path: null,
        action: 'proxy',
        pre_script: null,
        script: null,
        post_script: null,
        conditions: {
          all_of: [{ source: { type: 'Header', key: 'x-real-backend' }, operator: { type: 'Eq', value: 'true' } }],
          any_of: [],
        },
        response: { status: 200, headers: [], body: [{ type: 'Literal', value: '' }], chaos: null },
      },
    ],
  };

  async function init() {
    restoreAuth();

    try {
      const status = await getAuthStatus();
      auth.enabled = status.enabled;
      auth.showResetButton = status.show_reset_button;
    } catch {
      auth.enabled = false;
      auth.showResetButton = false;
    }

    try {
      const messagingStatus = await getMessagingStatus();
      messagingAvailable = !!messagingStatus?.available;
    } catch {
      // 404 on a binary built without the "messaging-kafka" feature: the expected default, not an error to show.
      messagingAvailable = false;
    }

    try {
      // 200, with a list that may be empty, on a binary built with "tcp-mock"; 404 otherwise, as for messaging above.
      await getTcpStatus();
      tcpAvailable = true;
    } catch {
      tcpAvailable = false;
    }

    const params = new URLSearchParams(window.location.search);
    const silentToken = params.get('token');
    if (silentToken && auth.enabled) {
      window.history.replaceState({}, '', window.location.pathname);
      try {
        const result = await validateToken(silentToken);
        setAuth(result);
      } catch {
        showNotification(t('Invalid token'), 'error');
      }
    }

    if (isLoggedIn()) {
      await loadData();
    }
    loading = false;
  }

  async function loadData() {
    try {
      services = await getServices();
      try {
        groups = await getGroups();
      } catch {
        groups = [];
      }
    } catch (e) {
      showNotification(t('Loading error: {0}', e.message), 'error');
    }
  }

  async function handleLogin() {
    await loadData();
  }

  function handleLogout() {
    logout();
    services = [];
    groups = [];
    view = 'list';
    selectedService = null;
    selectedServiceGroup = null;
  }

  async function handleToggle(name, isMocked, groupName) {
    try {
      const updated = await toggleService(name, groupName, isMocked);
      services = services.map((s) => (s.name === name && s.group_name === groupName ? updated : s));
      showNotification(t('{0}: {1} mode on', name, isMocked ? 'mock' : 'proxy'), 'success');
    } catch (e) {
      showNotification(t('Error: {0}', e.message), 'error');
    }
  }

  let clonedService = $state(null);

  function handleSelect(name, groupName) {
    selectedService = name;
    selectedServiceGroup = groupName ?? null;
    view = 'detail';
  }
  function handleBack() {
    selectedService = null;
    selectedServiceGroup = null;
    clonedService = null;
    view = 'list';
  }
  function handleCloneService(svc) {
    clonedService = { ...JSON.parse(JSON.stringify(svc)), name: t('{0}-copy', svc.name) };
    view = 'add';
  }
  // `previousGroupName` is the group before the change (ServiceDetail and GroupManager read it before calling the
  // API): it finds the entry to replace even when `updated.group_name` changed (a move to another group), which
  // `updated.name` alone could not.
  function handleServiceUpdate(updated, previousGroupName = updated.group_name) {
    services = services.map((s) => (s.name === updated.name && s.group_name === previousGroupName ? updated : s));
    selectedService = updated.name;
    selectedServiceGroup = updated.group_name ?? null;
  }
  function handleServiceDelete(name, groupName) {
    view = 'list';
    selectedService = null;
    selectedServiceGroup = null;
    services = services.filter((s) => !(s.name === name && s.group_name === (groupName ?? null)));
  }

  async function handleAddService(svc) {
    try {
      const result = await createService(svc);
      services = [...services, result];
      view = 'detail';
      selectedService = result.name;
      selectedServiceGroup = result.group_name ?? null;
      showNotification(t('Service "{0}" created', result.name), 'success');
    } catch (e) {
      showNotification(t('Error: {0}', e.message), 'error');
      throw e;
    }
  }

  async function loadDemo() {
    try {
      const result = await createService(demoService);
      services = [...services, result];
      showNotification(t('Demo service loaded (users-api with a mock, a proxy and a Rhai script)'), 'success');
    } catch (e) {
      showNotification(t('Error: {0}', e.message), 'error');
    }
  }

  async function exportConfig() {
    try {
      const config = { services, groups };
      const json = JSON.stringify(config, null, 2);
      const blob = new Blob([json], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `mimicway-config-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
      showNotification(t('Configuration exported'), 'success');
    } catch (e) {
      showNotification(t('Export error: {0}', e.message), 'error');
    }
  }

  let fileInput = $state(null);
  let importPending = $state(null);
  async function importConfig() {
    fileInput?.click();
  }

  async function handleFileImport(e) {
    const file = e.target.files?.[0];
    if (!file) return;
    try {
      const text = await file.text();
      const config = JSON.parse(text);
      if (!config.services || !Array.isArray(config.services)) {
        throw new Error(t('Invalid format: "services" expected'));
      }
      if (!config.groups) config.groups = [];
      const canReplace = !auth.enabled || auth.isSuperAdmin;
      if (canReplace) {
        importPending = config;
      } else {
        await doImportMerge(config);
      }
    } catch (e) {
      showNotification(t('Import error: {0}', e.message), 'error');
    }
    e.target.value = '';
  }

  async function doImportReplace(config) {
    importPending = null;
    try {
      await putConfig(config);
      await loadData();
      showNotification(t('Configuration replaced ({0} services)', config.services.length), 'success');
      view = 'list';
      selectedService = null;
      selectedServiceGroup = null;
    } catch (e) {
      showNotification(t('Import error: {0}', e.message), 'error');
    }
  }

  async function doImportMerge(config) {
    importPending = null;
    try {
      let addedGroups = 0;
      for (const grp of config.groups || []) {
        if (!groups.some((g) => g.name === grp.name)) {
          const created = await createGroup(grp);
          groups = [...groups, created];
          addedGroups++;
        }
      }
      let added = 0;
      for (const svc of config.services) {
        // Matched on name and group: two services of the same name in different groups are different, and a merge
        // comparing names only would skip a service that is new.
        if (!services.some((s) => s.name === svc.name && s.group_name === (svc.group_name ?? null))) {
          const result = await createService(svc);
          services = [...services, result];
          added++;
        }
      }
      showNotification(t('{0} service(s) and {1} group(s) added', added, addedGroups), 'success');
      view = 'list';
      selectedService = null;
      selectedServiceGroup = null;
    } catch (e) {
      showNotification(t('Import error: {0}', e.message), 'error');
    }
  }

  function showNotification(message, type) {
    notification = { message, type, visible: true };
    setTimeout(() => {
      notification = { ...notification, visible: false };
    }, 4000);
  }

  async function handleReset() {
    resetPending = false;
    try {
      await resetConfig();
      services = [];
      selectedService = null;
      selectedServiceGroup = null;
      view = 'list';
      showNotification(t('Configuration reset: every service removed'), 'success');
    } catch (e) {
      showNotification(t('Reset error: {0}', e.message), 'error');
    }
  }

  let canShowReset = $derived(auth.enabled ? auth.isSuperAdmin : auth.showResetButton);
  // The server restores a backup for a super-admin only (anyone, as an anonymous super-admin, without authentication).
  let canRestoreBackups = $derived(!auth.enabled || auth.isSuperAdmin);

  $effect(() => {
    init();
  });

  let currentService = $derived(
    services.find((s) => s.name === selectedService && s.group_name === selectedServiceGroup) ?? null,
  );
  let availableGroupsList = $derived(groups.map((g) => ({ name: g.name, code: g.code })));
</script>

<a href="#main-content" class="sr-only skip-link">{t('Skip to main content')}</a>

{#if loading}
  <div class="loading" role="status"><p>{t('Loading...')}</p></div>
{:else if auth.enabled && !isLoggedIn()}
  <LoginForm onLogin={handleLogin} />
{:else}
  <header class="app-header">
    <div class="header-content">
      <button type="button" class="app-title-btn" onclick={handleBack} data-testid="app-title-button">
        <h1 class="app-title">Mimicway</h1>
      </button>
      <p class="app-subtitle">{t('Smart mock & proxy')}</p>
      <div class="header-actions">
        <button
          type="button"
          class="btn btn-sm btn-outline"
          onclick={() => (view = 'logs')}
          title={t('Request log')}
          data-testid="app-nav-logs-button">{t('Logs')}</button
        >
        {#if messagingAvailable}
          <button
            type="button"
            class="btn btn-sm btn-outline"
            onclick={() => (view = 'messaging')}
            title={t('Kafka message log')}
            data-testid="app-nav-messaging-button">{t('Kafka messages')}</button
          >
        {/if}
        {#if tcpAvailable}
          <button
            type="button"
            class="btn btn-sm btn-outline"
            onclick={() => (view = 'tcp')}
            title={t('Raw TCP mock')}
            data-testid="app-nav-tcp-button">{t('TCP mock')}</button
          >
        {/if}
        <button
          type="button"
          class="btn btn-sm btn-outline"
          onclick={() => (view = 'groups')}
          title={t('Group management')}
          data-testid="app-nav-groups-button">{t('Groups')}</button
        >
        <button
          type="button"
          class="btn btn-sm btn-outline"
          onclick={exportConfig}
          title={t('Download the configuration')}
          data-testid="app-export-button">{t('Export')}</button
        >
        <button
          type="button"
          class="btn btn-sm btn-outline"
          onclick={importConfig}
          title={t('Load a configuration')}
          data-testid="app-import-button">{t('Import')}</button
        >
        {#if canRestoreBackups}
          <button
            type="button"
            class="btn btn-sm btn-outline"
            onclick={() => (view = 'backups')}
            title={t('Restore a configuration backup')}
            data-testid="app-nav-backups-button">{t('Backups')}</button
          >
        {/if}
        {#if canShowReset}
          <button
            type="button"
            class="btn btn-sm btn-outline btn-danger-outline"
            onclick={() => (resetPending = true)}
            title={t('Remove every service')}
            data-testid="app-reset-button">{t('Reset')}</button
          >
        {/if}
        <label class="sr-only" for="app-language-select">{t('Language')}</label>
        <select
          id="app-language-select"
          class="language-select"
          value={getLocale()}
          onchange={(e) => setLocale(e.currentTarget.value)}
          data-testid="app-language-select"
        >
          {#each LOCALES as locale (locale.code)}
            <option value={locale.code}>{locale.label}</option>
          {/each}
        </select>
        <button
          type="button"
          class="btn btn-sm btn-outline"
          onclick={() => (darkMode = !darkMode)}
          title={darkMode ? t('Light mode') : t('Dark mode')}
          aria-label={darkMode ? t('Switch to light mode') : t('Switch to dark mode')}
          data-testid="app-theme-toggle-button"
        >
          {darkMode ? t('Light') : t('Dark')}
        </button>
        {#if auth.enabled}
          <span class="user-badge" title={auth.isSuperAdmin ? t('Super-admin') : t('User')} data-testid="app-user-badge"
            >{auth.username}</span
          >
          <button type="button" class="btn btn-sm btn-outline" onclick={handleLogout} data-testid="app-logout-button"
            >{t('Log out')}</button
          >
        {/if}
        <input
          type="file"
          accept=".json"
          style="display:none"
          bind:this={fileInput}
          onchange={handleFileImport}
          data-testid="app-import-file-input"
        />
      </div>
    </div>
  </header>

  {#if view !== 'list'}
    <nav class="breadcrumb" aria-label={t('Breadcrumb')}>
      <ol>
        <li>
          <button type="button" class="breadcrumb-link" onclick={handleBack} data-testid="app-breadcrumb-services-link"
            >{t('Services')}</button
          >
        </li>
        <li aria-current="page">
          {#if view === 'logs'}{t('Request log')}
          {:else if view === 'messaging'}{t('Kafka messages')}
          {:else if view === 'tcp'}{t('Raw TCP mock')}
          {:else if view === 'groups'}{t('Service groups')}
          {:else if view === 'backups'}{t('Configuration backups')}
          {:else if view === 'add'}{t('Add a service')}
          {:else if view === 'detail' && currentService}{t('Details: {0}', currentService.name)}
          {/if}
        </li>
      </ol>
    </nav>
  {/if}

  {#if importPending}
    <div
      class="modal-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={t('Import mode')}
      data-testid="app-import-modal"
    >
      <div class="modal-content">
        <div class="modal-header">
          <h3>{t('Import the configuration')}</h3>
          <button
            type="button"
            class="btn-close"
            onclick={() => (importPending = null)}
            aria-label={t('Close')}
            data-testid="app-import-modal-close-button">&#10005;</button
          >
        </div>
        <p>
          {t(
            '{0} service(s) and {1} group(s) found in the file.',
            importPending.services.length,
            importPending.groups?.length ?? 0,
          )}
        </p>
        <div class="import-actions">
          <button
            type="button"
            class="btn btn-primary"
            onclick={() => doImportReplace(importPending)}
            data-testid="app-import-replace-button"
          >
            {t('Replace everything')}
          </button>
          <button
            type="button"
            class="btn btn-outline"
            onclick={() => doImportMerge(importPending)}
            data-testid="app-import-merge-button"
          >
            {t('Merge (add what is missing)')}
          </button>
          <button
            type="button"
            class="btn btn-secondary"
            onclick={() => (importPending = null)}
            data-testid="app-import-cancel-button"
          >
            {t('Cancel')}
          </button>
        </div>
      </div>
    </div>
  {/if}

  <ConfirmDialog
    open={resetPending}
    title={t('Reset the configuration')}
    message={t(
      'Remove every service and start from scratch? A protected backup is kept for 30 days, but this action has heavy consequences.',
    )}
    confirmLabel={t('Reset everything')}
    confirmKeyword="RESET"
    onConfirm={handleReset}
    onCancel={() => (resetPending = false)}
  />

  <main id="main-content" class="app-main">
    <Notification message={notification.message} type={notification.type} visible={notification.visible} />

    {#if view === 'logs'}
      <RequestLog />
    {:else if view === 'messaging'}
      <MessagingLog onNotify={showNotification} onBack={handleBack} />
    {:else if view === 'tcp'}
      <TcpServiceManager onNotify={showNotification} onBack={handleBack} />
    {:else if view === 'groups'}
      <GroupManager
        {services}
        authEnabled={auth.enabled}
        onNotify={showNotification}
        onBack={handleBack}
        onServiceUpdate={handleServiceUpdate}
        onGroupsChange={(g) => (groups = g)}
      />
    {:else if view === 'backups'}
      <BackupManager onNotify={showNotification} onBack={handleBack} />
    {:else if view === 'add'}
      <ServiceForm
        service={clonedService}
        existingNames={services.map((s) => s.name)}
        availableGroups={availableGroupsList}
        authEnabled={auth.enabled}
        onSave={handleAddService}
        onCancel={handleBack}
      />
    {:else if view === 'detail' && currentService}
      <ServiceDetail
        service={currentService}
        availableGroups={availableGroupsList}
        onBack={handleBack}
        onUpdate={handleServiceUpdate}
        onDelete={handleServiceDelete}
        onNotify={showNotification}
      />
    {:else}
      <div class="list-header">
        <h2>{t('Services')}</h2>
        <button
          type="button"
          class="btn btn-primary"
          onclick={() => (view = 'add')}
          data-testid="app-add-service-button">{t('+ Add a service')}</button
        >
      </div>
      <ServiceList {services} {groups} onToggle={handleToggle} onSelect={handleSelect} onClone={handleCloneService} />
      {#if services.length === 0}
        <div class="demo-section">
          <button type="button" class="btn btn-outline btn-demo" onclick={loadDemo} data-testid="app-load-demo-button">
            {t('Load an example')}
          </button>
          <span class="field-hint"
            >{t(
              'users-api service with a mock (fake data, a Rhai script with a 4 in 5 ratio) and a conditional proxy.',
            )}</span
          >
        </div>
      {/if}
    {/if}
  </main>
{/if}

<style>
  :global(.skip-link:focus) {
    position: fixed;
    top: 0;
    left: 0;
    z-index: var(--z-skip-link);
    width: auto;
    height: auto;
    clip: auto;
    padding: var(--space-3) var(--space-6);
    background: var(--color-primary);
    color: var(--color-on-primary);
    font-weight: var(--weight-strong);
    text-decoration: none;
  }

  .app-header {
    background: var(--color-surface);
    border-bottom: var(--line-thin) solid var(--color-border);
    padding: var(--space-3) var(--space-6);
  }
  .header-content {
    max-width: 60rem;
    margin: 0 auto;
    display: flex;
    align-items: baseline;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .header-actions {
    margin-left: auto;
    display: flex;
    gap: var(--space-2);
    align-items: center;
    flex-wrap: wrap;
  }
  .app-title-btn {
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
  }
  .app-title {
    font-size: var(--text-3xl);
    margin: 0;
    color: var(--color-text);
  }
  /* The mark of Phasme: a short dashed stem, the line an imitation is drawn with. */
  .app-title::before {
    content: '';
    display: inline-block;
    height: 0.8em;
    margin-right: var(--space-2);
    border-left: var(--line-stem) dashed var(--color-mock);
    vertical-align: -0.05em;
  }
  .app-subtitle {
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--text-m);
  }
  .app-main {
    max-width: 60rem;
    margin: var(--space-6) auto;
    padding: 0 var(--space-6);
  }

  .breadcrumb {
    max-width: 60rem;
    margin: 0 auto;
    padding: var(--space-2) var(--space-6);
  }
  .breadcrumb ol {
    list-style: none;
    display: flex;
    align-items: center;
    gap: var(--space-1-5);
    margin: 0;
    padding: 0;
    font-size: var(--text-m);
  }
  .breadcrumb li {
    display: flex;
    align-items: center;
    gap: var(--space-1-5);
    color: var(--color-text-muted);
  }
  .breadcrumb li:not(:last-child)::after {
    content: '/';
    color: var(--color-text-muted);
  }
  .breadcrumb li[aria-current='page'] {
    color: var(--color-text);
    font-weight: var(--weight-strong);
  }
  .breadcrumb-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--color-primary);
    cursor: pointer;
    font: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .breadcrumb-link:hover {
    color: var(--color-primary-hover);
  }

  .import-actions {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
    margin-top: var(--space-4);
  }
  .loading {
    text-align: center;
    padding: var(--space-12);
    color: var(--color-text-muted);
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

  .demo-section {
    text-align: center;
    margin-top: var(--space-4);
  }
  .btn-demo {
    font-size: var(--text-l);
    padding: var(--space-3) var(--space-6);
  }

  .language-select {
    font: inherit;
    font-size: var(--text-s);
    color: var(--color-text);
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    padding: var(--space-1) var(--space-2);
  }

  .user-badge {
    font-size: var(--text-s);
    font-weight: var(--weight-strong);
    color: var(--color-primary);
    background: var(--color-bg);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-1) var(--space-3);
  }
</style>
