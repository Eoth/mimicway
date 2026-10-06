<script>
  import ServiceGroup from './ServiceGroup.svelte';
  import { t, tCount } from '../i18n.svelte.js';
  import { getExpandedGroupKeys, setGroupExpanded, toggleGroupExpanded } from '../group-expansion-state.svelte.js';

  let { services = [], groups = [], onToggle = () => {}, onSelect = () => {}, onClone = () => {} } = $props();

  let search = $state('');
  let initialized = $state(false);

  let filtered = $derived(
    search.trim() === ''
      ? services
      : services.filter(
          (s) =>
            s.name.toLowerCase().includes(search.toLowerCase()) ||
            (s.listen_path || '').toLowerCase().includes(search.toLowerCase()) ||
            s.real_target_url.toLowerCase().includes(search.toLowerCase()) ||
            (s.group_name || '').toLowerCase().includes(search.toLowerCase()),
        ),
  );

  let grouped = $derived(() => {
    const map = new Map();
    for (const s of filtered) {
      const key = s.group_name || '__ungrouped__';
      if (!map.has(key)) map.set(key, []);
      map.get(key).push(s);
    }
    const entries = [...map.entries()].sort((a, b) => {
      if (a[0] === '__ungrouped__') return 1;
      if (b[0] === '__ungrouped__') return -1;
      return a[0].localeCompare(b[0]);
    });
    return entries;
  });

  $effect(() => {
    if (!initialized && services.length > 0) {
      initialized = true;
    }
  });

  let effectiveExpanded = $derived(search.trim() ? new Set(grouped().map(([key]) => key)) : getExpandedGroupKeys());

  // Leaving the list to edit or clone a service unfolds its group for good, so that it is still open on the way back,
  // even when only the search had unfolded it (the search never writes to the shared expansion state).
  function handleSelect(name, groupName) {
    setGroupExpanded(groupName || '__ungrouped__', true);
    onSelect(name, groupName);
  }

  function handleClone(service) {
    setGroupExpanded(service.group_name || '__ungrouped__', true);
    onClone(service);
  }

  function groupDisplayName(key) {
    return key === '__ungrouped__' ? t('No group') : key;
  }

  function groupId(key) {
    return key === '__ungrouped__' ? 'ungrouped' : key.replace(/[^a-zA-Z0-9-]/g, '_');
  }

  function groupCodeFor(key) {
    if (key === '__ungrouped__') return '';
    const g = groups.find((gr) => gr.name === key);
    return g?.code ?? '';
  }
</script>

<section aria-label={t('Service list')}>
  {#if services.length === 0}
    <div class="empty-state" role="status">
      <p class="empty-title">{t('No service configured')}</p>
      <p>{t('Add a service to start mocking or proxying routes.')}</p>
    </div>
  {:else}
    <div class="search-bar">
      <label for="service-search" class="sr-only">{t('Search a service')}</label>
      <input
        id="service-search"
        type="search"
        bind:value={search}
        placeholder={t('Search by name, path, URL or group...')}
        aria-label={t('Search a service')}
        data-testid="service-list-search-input"
      />
      {#if search.trim()}
        <span class="search-count" role="status" aria-live="polite" data-testid="service-list-search-count">
          {tCount(filtered.length, '{0} / {1} service', '{0} / {1} services', services.length)}
        </span>
      {/if}
    </div>

    {#if filtered.length === 0}
      <div class="no-results" role="status" data-testid="service-list-no-results">
        <p>{t('No service matches “{0}”', search)}</p>
      </div>
    {:else}
      <div class="groups-container">
        {#each grouped() as [key, groupServices] (key)}
          <ServiceGroup
            groupName={groupDisplayName(key)}
            groupId={groupId(key)}
            groupCode={groupCodeFor(key)}
            services={groupServices}
            expanded={effectiveExpanded.has(key)}
            onToggleGroup={() => toggleGroupExpanded(key)}
            {onToggle}
            onSelect={handleSelect}
            onClone={handleClone}
          />
        {/each}
      </div>
    {/if}
  {/if}
</section>

<style>
  .groups-container {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .search-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
  }

  .search-bar input {
    flex: 1;
    padding: var(--space-3) var(--space-4);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-l);
    font-family: inherit;
    background: var(--color-surface);
    color: var(--color-text);
  }

  .search-count {
    font-size: var(--text-m);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .empty-state {
    text-align: center;
    padding: var(--space-12) var(--space-4);
    color: var(--color-text-muted);
    background: var(--color-surface);
    border: var(--line-thick) dashed var(--color-border);
    border-radius: var(--radius-m);
  }

  .empty-title {
    font-weight: var(--weight-strong);
    font-size: var(--text-xl);
    color: var(--color-text);
    margin-bottom: var(--space-1);
  }

  .empty-state p {
    margin: var(--space-1) 0;
  }

  .no-results {
    text-align: center;
    padding: var(--space-8) var(--space-4);
    color: var(--color-text-muted);
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
  }

  .no-results p {
    margin: 0;
  }
</style>
