<script>
  import { getLogs } from '../api.js';
  import { formatDateTimePrecise } from '../format-date.js';
  import { t, tCount } from '../i18n.svelte.js';

  let logs = $state([]);
  let loading = $state(true);
  let detailLog = $state(null);

  let filterService = $state('');
  let filterMode = $state('');
  let filterStatus = $state('');
  let filterText = $state('');
  let filterTime = $state('');

  async function refresh() {
    loading = true;
    try {
      logs = await getLogs(200);
    } catch (e) {
      logs = [];
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    refresh();
  });

  let serviceNames = $derived([...new Set(logs.map((l) => l.service_name))].sort());

  let filteredLogs = $derived(() => {
    const now = Date.now();
    return logs.filter((l) => {
      if (filterService && l.service_name !== filterService) return false;
      if (filterMode && l.mode !== filterMode) return false;

      if (filterStatus) {
        const s = l.status;
        if (filterStatus === '2xx' && (s < 200 || s >= 300)) return false;
        if (filterStatus === '3xx' && (s < 300 || s >= 400)) return false;
        if (filterStatus === '4xx' && (s < 400 || s >= 500)) return false;
        if (filterStatus === '5xx' && s < 500) return false;
      }

      if (filterText && !l.path.toLowerCase().includes(filterText.toLowerCase())) return false;

      if (filterTime) {
        const age = now - l.timestamp;
        if (filterTime === '1m' && age > 60_000) return false;
        if (filterTime === '5m' && age > 300_000) return false;
        if (filterTime === '1h' && age > 3_600_000) return false;
      }

      return true;
    });
  });

  let activeFilterCount = $derived(
    [filterService, filterMode, filterStatus, filterText, filterTime].filter(Boolean).length,
  );

  function clearFilters() {
    filterService = '';
    filterMode = '';
    filterStatus = '';
    filterText = '';
    filterTime = '';
  }

  function modeBadge(mode) {
    if (mode === 'mock') return 'badge-mock';
    if (mode === 'proxy') return 'badge-proxy';
    return 'badge-error';
  }

  function openDetail(log) {
    detailLog = log;
  }
  function closeDetail() {
    detailLog = null;
  }
  function handleKeydown(e) {
    if (e.key === 'Escape') closeDetail();
  }
  function handleBackdrop(e) {
    if (e.target === e.currentTarget) closeDetail();
  }
</script>

<section class="log-section" aria-label={t('Request log')}>
  <div class="log-header">
    <h2>{t('Request log')}</h2>
    <div class="log-controls">
      <button type="button" class="btn btn-sm btn-outline" onclick={refresh} data-testid="request-log-refresh-button"
        >{t('Refresh')}</button
      >
    </div>
  </div>

  <div class="filters-bar">
    {#if serviceNames.length > 1}
      <select
        class="filter-select"
        bind:value={filterService}
        aria-label={t('Filter by service')}
        data-testid="request-log-filter-service"
      >
        <option value="">{t('All services')}</option>
        {#each serviceNames as sn}
          <option value={sn}>{sn}</option>
        {/each}
      </select>
    {/if}

    <select
      class="filter-select"
      bind:value={filterMode}
      aria-label={t('Filter by mode')}
      data-testid="request-log-filter-mode"
    >
      <option value="">{t('All modes')}</option>
      <option value="mock">{t('Mock')}</option>
      <option value="proxy">{t('Proxy')}</option>
      <option value="no-rule">{t('No rule')}</option>
    </select>

    <select
      class="filter-select"
      bind:value={filterStatus}
      aria-label={t('Filter by HTTP status')}
      data-testid="request-log-filter-status"
    >
      <option value="">{t('All statuses')}</option>
      <option value="2xx">{t('2xx (success)')}</option>
      <option value="3xx">{t('3xx (redirection)')}</option>
      <option value="4xx">{t('4xx (client error)')}</option>
      <option value="5xx">{t('5xx (server error)')}</option>
    </select>

    <select
      class="filter-select"
      bind:value={filterTime}
      aria-label={t('Filter by period')}
      data-testid="request-log-filter-time"
    >
      <option value="">{t('Any time')}</option>
      <option value="1m">{t('Last minute')}</option>
      <option value="5m">{t('Last 5 minutes')}</option>
      <option value="1h">{t('Last hour')}</option>
    </select>

    <input
      type="text"
      class="filter-search"
      bind:value={filterText}
      placeholder={t('Search a path...')}
      aria-label={t('Text search in the path')}
      data-testid="request-log-filter-search"
    />

    {#if activeFilterCount > 0}
      <button
        type="button"
        class="btn btn-sm btn-outline btn-clear"
        onclick={clearFilters}
        title={t('Clear every filter')}
        data-testid="request-log-clear-filters-button"
      >
        {t('Clear ({0})', activeFilterCount)}
      </button>
    {/if}
  </div>

  {#if loading}
    <p class="loading">{t('Loading...')}</p>
  {:else if filteredLogs().length === 0}
    <p class="empty">
      {#if activeFilterCount > 0}
        {t('No request matches the filters.')}
      {:else}
        {t('No request intercepted yet.')}
      {/if}
    </p>
  {:else}
    <p class="result-count">
      {activeFilterCount > 0
        ? tCount(filteredLogs().length, '{0} request (filtered)', '{0} requests (filtered)')
        : tCount(filteredLogs().length, '{0} request', '{0} requests')}
    </p>
    <div class="table-scroll">
      <table class="log-table" aria-label={t('Latest requests')}>
        <thead>
          <tr>
            <th>{t('Date/time')}</th>
            <th>{t('Service')}</th>
            <th>{t('Method')}</th>
            <th>{t('Path')}</th>
            <th>{t('Mode')}</th>
            <th>{t('Rule / target')}</th>
            <th>{t('Status')}</th>
            <th><span class="sr-only">{t('Actions')}</span></th>
          </tr>
        </thead>
        <tbody>
          {#each filteredLogs() as log, idx}
            <tr data-testid="request-log-row-{idx}">
              <td class="col-time">{formatDateTimePrecise(log.timestamp)}</td>
              <td><strong>{log.service_name}</strong></td>
              <td><span class="method-badge" data-method={log.method}>{log.method}</span></td>
              <td class="col-path"><code class="truncate" title={log.path}>{log.path}</code></td>
              <td><span class="badge {modeBadge(log.mode)}">{log.mode}</span></td>
              <td class="col-detail" translate="no"
                ><span class="truncate" title={log.rule_matched || log.target_url || '-'}
                  >{log.rule_matched || log.target_url || '-'}</span
                ></td
              >
              <td
                ><span class="status" class:status-ok={log.status < 400} class:status-err={log.status >= 400}
                  >{log.status}</span
                ></td
              >
              <td>
                <button
                  type="button"
                  class="btn-detail"
                  onclick={() => openDetail(log)}
                  aria-label={t('Show the details of the request {0}', log.path)}
                  title={t('Details')}
                  data-testid="request-log-detail-button-{idx}">&#8942;</button
                >
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

{#if detailLog}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="modal-overlay"
    role="dialog"
    aria-modal="true"
    aria-label={t('Request details')}
    tabindex="-1"
    onkeydown={handleKeydown}
    onclick={handleBackdrop}
    data-testid="request-log-detail-modal"
  >
    <div class="modal-content" role="document">
      <div class="modal-header">
        <h3>{t('Request details')}</h3>
        <button
          type="button"
          class="btn-close"
          onclick={closeDetail}
          aria-label={t('Close')}
          data-testid="request-log-detail-modal-close-button">&#10005;</button
        >
      </div>
      <dl class="detail-list">
        <div class="detail-row">
          <dt>{t('Date/time')}</dt>
          <dd>{formatDateTimePrecise(detailLog.timestamp)}</dd>
        </div>
        <div class="detail-row">
          <dt>{t('Service')}</dt>
          <dd>{detailLog.service_name}</dd>
        </div>
        <div class="detail-row">
          <dt>{t('Method')}</dt>
          <dd><span class="method-badge" data-method={detailLog.method}>{detailLog.method}</span></dd>
        </div>
        <div class="detail-row">
          <dt>{t('Path')}</dt>
          <dd class="dd-mono">{detailLog.path}</dd>
        </div>
        <div class="detail-row">
          <dt>{t('Mode')}</dt>
          <dd><span class="badge {modeBadge(detailLog.mode)}">{detailLog.mode}</span></dd>
        </div>
        {#if detailLog.rule_matched}
          <div class="detail-row">
            <dt>{t('Matched rule')}</dt>
            <dd class="dd-mono">{detailLog.rule_matched}</dd>
          </div>
        {/if}
        {#if detailLog.target_url}
          <div class="detail-row">
            <dt>{t('Target URL')}</dt>
            <dd class="dd-mono dd-break">{detailLog.target_url}</dd>
          </div>
        {/if}
        <div class="detail-row">
          <dt>{t('Status')}</dt>
          <dd>
            <span class="status" class:status-ok={detailLog.status < 400} class:status-err={detailLog.status >= 400}
              >{detailLog.status}</span
            >
          </dd>
        </div>
      </dl>
      <div class="modal-footer">
        <button type="button" class="btn btn-sm btn-secondary" onclick={closeDetail}>{t('Close')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .log-section {
    margin-top: var(--space-4);
  }
  .log-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-3);
  }
  .log-header h2 {
    margin: 0;
    font-size: var(--text-2xl);
  }
  .log-controls {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  .filters-bar {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    flex-wrap: wrap;
    margin-bottom: var(--space-3);
    padding: var(--space-3) var(--space-3);
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
  }
  .filter-select {
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
    background: var(--color-bg);
    color: var(--color-text);
  }
  .filter-search {
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
    background: var(--color-bg);
    color: var(--color-text);
    min-width: 10rem;
    flex: 1;
  }
  .filter-search::placeholder {
    color: var(--color-text-muted);
  }
  .btn-clear {
    color: var(--color-danger);
    border-color: var(--color-danger);
  }
  .btn-clear:hover {
    background: var(--color-danger);
    color: var(--color-on-danger);
  }

  .result-count {
    font-size: var(--text-s);
    color: var(--color-text-muted);
    margin: 0 0 var(--space-2);
  }

  .loading,
  .empty {
    color: var(--color-text-muted);
    text-align: center;
    padding: var(--space-8);
  }

  .log-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-s);
  }
  .log-table th {
    background: var(--color-bg);
    font-weight: var(--weight-strong);
    text-align: left;
    padding: var(--space-2);
    border-bottom: var(--line-thick) solid var(--color-border);
  }
  .log-table td {
    padding: var(--space-1-5) var(--space-2);
    border-bottom: var(--line-thin) solid var(--color-border);
    vertical-align: middle;
  }
  .col-time {
    white-space: nowrap;
    color: var(--color-text-muted);
    font-family: var(--font-code);
  }
  /* A table cell ignores max-width in automatic table layout, so long paths and target URLs widened the table past
     its container; the truncation sits on an inner block instead. */
  .truncate {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .col-path .truncate {
    max-width: 20rem;
  }
  .col-path code {
    background: none;
    padding: 0;
    font-size: var(--text-s);
  }
  .col-detail {
    font-size: var(--text-s);
    color: var(--color-text-muted);
    cursor: default;
  }
  .col-detail .truncate {
    max-width: 12rem;
  }

  .status {
    font-weight: var(--weight-strong);
    font-family: var(--font-code);
  }
  .status-ok {
    color: var(--color-success);
  }
  .status-err {
    color: var(--color-danger);
  }

  .btn-detail {
    background: none;
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    padding: var(--space-0-5) var(--space-1-5);
    font-size: var(--text-m);
    cursor: pointer;
    color: var(--color-text-muted);
    line-height: var(--leading-none);
    letter-spacing: 0.05em;
  }
  .btn-detail:hover {
    background: var(--color-bg);
    color: var(--color-text);
  }

  .detail-list {
    margin: 0;
    padding: 0;
  }
  .detail-row {
    display: flex;
    gap: var(--space-4);
    padding: var(--space-2) 0;
    border-bottom: var(--line-thin) solid var(--color-border);
  }
  .detail-row:last-child {
    border-bottom: none;
  }
  .detail-row dt {
    font-weight: var(--weight-strong);
    font-size: var(--text-s);
    min-width: 7rem;
    flex-shrink: 0;
    color: var(--color-text-muted);
  }
  .detail-row dd {
    margin: 0;
    font-size: var(--text-m);
    word-break: break-word;
  }
  .dd-mono {
    font-family: var(--font-code);
    font-size: var(--text-s);
  }
  .dd-break {
    word-break: break-all;
  }
</style>
