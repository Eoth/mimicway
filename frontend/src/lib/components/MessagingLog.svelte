<script>
  // The log of the Kafka messages handled (the server's "messaging-kafka" feature), with the layout and accessibility
  // of RequestLog.svelte (table, filters, detail dialog), adapted to messages: a direction (incoming, outgoing) instead
  // of an HTTP mode, and a "truncated" badge when the body exceeded MESSAGE_LOG_MAX_BODY_SIZE on the server
  // (src/messaging/message_log.rs). Its "Simulate a message" panel (POST /api/messaging/simulate) tries a messaging
  // rule without a real Kafka producer; the end-to-end tests use it too (e2e/messaging.spec.js).
  import { getMessagingLogs, simulateMessage } from '../api.js';
  import { formatDateTimePrecise } from '../format-date.js';
  import { t, tCount } from '../i18n.svelte.js';

  let { onNotify = () => {}, onBack = () => {} } = $props();

  let logs = $state([]);
  let loading = $state(true);
  let detailLog = $state(null);

  let filterDirection = $state('');
  let filterMatched = $state('');
  let filterText = $state('');

  let simTopic = $state('');
  let simPayload = $state('{\n  "type": "order.created"\n}');
  let simulating = $state(false);

  async function refresh() {
    loading = true;
    try {
      logs = await getMessagingLogs(200);
    } catch (e) {
      logs = [];
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    refresh();
  });

  let filteredLogs = $derived(() => {
    return logs.filter((l) => {
      if (filterDirection && l.direction !== filterDirection) return false;
      if (filterMatched === 'matched' && !l.matched) return false;
      if (filterMatched === 'unmatched' && l.matched) return false;
      if (filterText && !l.topic.toLowerCase().includes(filterText.toLowerCase())) return false;
      return true;
    });
  });

  let activeFilterCount = $derived([filterDirection, filterMatched, filterText].filter(Boolean).length);

  function clearFilters() {
    filterDirection = '';
    filterMatched = '';
    filterText = '';
  }

  function directionBadge(direction) {
    // The reply Mimicway publishes is the imitation, drawn like a mock; a message received is information.
    return direction === 'out' ? 'badge-mock' : 'badge-info';
  }

  function directionLabel(direction) {
    return direction === 'out' ? t('Outgoing (reply)') : t('Incoming');
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

  async function handleSimulate() {
    if (!simTopic.trim()) {
      onNotify(t('A topic is required to simulate a message.'), 'error');
      return;
    }
    simulating = true;
    try {
      await simulateMessage(simTopic.trim(), simPayload);
      onNotify(t('Message simulated on "{0}"', simTopic.trim()), 'success');
      await refresh();
    } catch (e) {
      onNotify(t('Simulation error: {0}', e.message), 'error');
    } finally {
      simulating = false;
    }
  }
</script>

<section class="log-section" aria-label={t('Kafka message log')}>
  <div class="log-header">
    <h2>{t('Kafka messages')}</h2>
    <div class="log-controls">
      <button type="button" class="btn btn-sm btn-outline" onclick={refresh} data-testid="messaging-log-refresh-button"
        >{t('Refresh')}</button
      >
      <button type="button" class="btn btn-outline btn-sm" onclick={onBack} data-testid="messaging-log-back-button"
        >{t('Back')}</button
      >
    </div>
  </div>

  <div class="simulate-panel">
    <h3>{t('Simulate an incoming message')}</h3>
    <p class="field-hint">
      {t(
        'No real Kafka producer needed: runs the same pipeline (matching, rendering, log, publication if any) as a real message received on the listened topic.',
      )}
    </p>
    <div class="simulate-fields">
      <label class="form-field-inline">
        <span>{t('Topic')}</span>
        <input
          type="text"
          bind:value={simTopic}
          placeholder="orders.in"
          aria-label={t('Topic of the simulated message')}
          data-testid="messaging-log-sim-topic-input"
        />
      </label>
      <label class="form-field-inline form-field-inline-grow">
        <span>{t('Payload')}</span>
        <textarea
          bind:value={simPayload}
          rows="3"
          aria-label={t('Body of the simulated message')}
          data-testid="messaging-log-sim-payload-textarea"></textarea>
      </label>
      <button
        type="button"
        class="btn btn-primary btn-sm"
        disabled={simulating}
        onclick={handleSimulate}
        data-testid="messaging-log-simulate-button"
      >
        {simulating ? t('Sending...') : t('Simulate')}
      </button>
    </div>
  </div>

  <div class="filters-bar">
    <select
      class="filter-select"
      bind:value={filterDirection}
      aria-label={t('Filter by direction')}
      data-testid="messaging-log-filter-direction"
    >
      <option value="">{t('Every direction')}</option>
      <option value="in">{t('Incoming')}</option>
      <option value="out">{t('Outgoing (reply)')}</option>
    </select>

    <select
      class="filter-select"
      bind:value={filterMatched}
      aria-label={t('Filter by match status')}
      data-testid="messaging-log-filter-matched"
    >
      <option value="">{t('Every status')}</option>
      <option value="matched">{t('Matched')}</option>
      <option value="unmatched">{t('Not matched')}</option>
    </select>

    <input
      type="text"
      class="filter-search"
      bind:value={filterText}
      placeholder={t('Search a topic...')}
      aria-label={t('Text search in the topic')}
      data-testid="messaging-log-filter-search"
    />

    {#if activeFilterCount > 0}
      <button
        type="button"
        class="btn btn-sm btn-outline btn-clear"
        onclick={clearFilters}
        title={t('Clear every filter')}
        data-testid="messaging-log-clear-filters-button"
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
        {t('No message matches the filters.')}
      {:else}
        {t('No Kafka message processed yet.')}
      {/if}
    </p>
  {:else}
    <p class="result-count">
      {activeFilterCount > 0
        ? tCount(filteredLogs().length, '{0} message (filtered)', '{0} messages (filtered)')
        : tCount(filteredLogs().length, '{0} message', '{0} messages')}
    </p>
    <div class="table-scroll">
      <table class="log-table" aria-label={t('Latest Kafka messages')}>
        <thead>
          <tr>
            <th>{t('Date/time')}</th>
            <th>{t('Direction')}</th>
            <th>{t('Topic')}</th>
            <th>{t('Service / rule')}</th>
            <th>{t('Match')}</th>
            <th>{t('Size')}</th>
            <th><span class="sr-only">{t('Actions')}</span></th>
          </tr>
        </thead>
        <tbody>
          {#each filteredLogs() as log, idx}
            <tr data-testid="messaging-log-row-{idx}">
              <td class="col-time">{formatDateTimePrecise(log.timestamp)}</td>
              <td><span class="badge {directionBadge(log.direction)}">{directionLabel(log.direction)}</span></td>
              <td class="col-path"><code>{log.topic}</code></td>
              <td class="col-detail" title={log.rule_matched ? `${log.service_name} / ${log.rule_matched}` : '-'}>
                {log.rule_matched ? `${log.service_name} / ${log.rule_matched}` : '-'}
              </td>
              <td>
                <span class="badge {log.matched ? 'badge-success' : 'badge-error'}"
                  >{log.matched ? t('Matches') : t('Does not match')}</span
                >
                {#if log.body_truncated}
                  <span class="badge badge-testing">{t('Truncated')}</span>
                {/if}
              </td>
              <td class="col-size">{t('{0} B', log.body_size_bytes)}</td>
              <td>
                <button
                  type="button"
                  class="btn-detail"
                  onclick={() => openDetail(log)}
                  aria-label={t('Show the details of the message {0}', log.topic)}
                  title={t('Details')}
                  data-testid="messaging-log-detail-button-{idx}">&#8942;</button
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
  <!-- The dialog itself only listens for Escape and for a click on its backdrop; its buttons are the controls. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="modal-overlay"
    role="dialog"
    aria-modal="true"
    aria-label={t('Message details')}
    tabindex="-1"
    onkeydown={handleKeydown}
    onclick={handleBackdrop}
    data-testid="messaging-log-detail-modal"
  >
    <div class="modal-content" role="document">
      <div class="modal-header">
        <h3>{t('Message details')}</h3>
        <button
          type="button"
          class="btn-close"
          onclick={closeDetail}
          aria-label={t('Close')}
          data-testid="messaging-log-detail-modal-close-button">&#10005;</button
        >
      </div>
      <dl class="detail-list">
        <div class="detail-row">
          <dt>{t('Date/time')}</dt>
          <dd>{formatDateTimePrecise(detailLog.timestamp)}</dd>
        </div>
        <div class="detail-row">
          <dt>{t('Direction')}</dt>
          <dd>
            <span class="badge {directionBadge(detailLog.direction)}">{directionLabel(detailLog.direction)}</span>
          </dd>
        </div>
        <div class="detail-row">
          <dt>{t('Topic')}</dt>
          <dd class="dd-mono">{detailLog.topic}</dd>
        </div>
        {#if detailLog.service_name}
          <div class="detail-row">
            <dt>{t('Service')}</dt>
            <dd>{detailLog.service_name}</dd>
          </div>
        {/if}
        {#if detailLog.rule_matched}
          <div class="detail-row">
            <dt>{t('Matched rule')}</dt>
            <dd class="dd-mono">{detailLog.rule_matched}</dd>
          </div>
        {/if}
        <div class="detail-row">
          <dt>{t('Actual size')}</dt>
          <dd>
            {t('{0} bytes', detailLog.body_size_bytes)}{#if detailLog.body_truncated}
              <span class="badge badge-testing">{t('Truncated in the preview')}</span>{/if}
          </dd>
        </div>
        <div class="detail-row">
          <dt>{detailLog.body_truncated ? t('Body (truncated preview)') : t('Body')}</dt>
          <dd class="dd-mono dd-break dd-body">{detailLog.body_preview}</dd>
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

  .simulate-panel {
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-3) var(--space-5);
    margin-bottom: var(--space-4);
  }
  .simulate-panel h3 {
    margin: 0 0 var(--space-1);
    font-size: var(--text-l);
  }
  .simulate-fields {
    display: flex;
    gap: var(--space-3);
    align-items: flex-end;
    flex-wrap: wrap;
    margin-top: var(--space-2);
  }
  .form-field-inline {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-s);
  }
  .form-field-inline-grow {
    flex: 1;
    min-width: 14rem;
  }
  .form-field-inline input,
  .form-field-inline textarea {
    padding: var(--space-1-5) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    background: var(--color-bg);
    color: var(--color-text);
    font-family: inherit;
    font-size: var(--text-s);
  }
  .form-field-inline textarea {
    font-family: var(--font-code);
    resize: vertical;
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
  .col-path {
    max-width: 16rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .col-path code {
    background: none;
    padding: 0;
    font-size: var(--text-s);
  }
  .col-detail {
    max-width: 14rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }
  .col-size {
    white-space: nowrap;
    font-family: var(--font-code);
    color: var(--color-text-muted);
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
  .dd-body {
    white-space: pre-wrap;
    max-height: 12rem;
    overflow-y: auto;
  }
</style>
