<script>
  import ToggleSwitch from './ToggleSwitch.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import UrlHealthBadge from './UrlHealthBadge.svelte';
  import { buildServiceTestUrl } from '../service-url.js';
  import { t } from '../i18n.svelte.js';

  let { service, groupCode = '', onToggle = () => {}, onSelect = () => {}, onClone = () => {} } = $props();

  let testUrl = $derived(buildServiceTestUrl({ name: service.name, listenPath: service.listen_path, groupCode }));
</script>

<article
  class="service-card"
  class:mocked={service.is_mocked}
  aria-label={t('Service {0}', service.name)}
  data-testid="service-card-{service.name}"
>
  <div class="card-header">
    <div class="card-info">
      <h3 class="card-title">{service.name}</h3>
      <StatusBadge active={service.is_mocked} />
    </div>
    <ToggleSwitch
      label={t('Mock {0}', service.name)}
      name="mock-{service.name}"
      checked={service.is_mocked}
      onchange={(val) => onToggle(service.name, val, service.group_name)}
    />
  </div>
  <div class="card-details" id="desc-{service.name}">
    <dl>
      <div class="detail-row">
        <dt>{t('Test URL')}</dt>
        <dd><code>{testUrl}</code></dd>
      </div>
      {#if service.real_target_url?.trim()}
        <div class="detail-row">
          <dt>{t('Target')}</dt>
          <dd><code>{service.real_target_url}</code></dd>
        </div>
        <div class="detail-row">
          <dt>{t('Availability')}</dt>
          <dd><UrlHealthBadge serviceName={service.name} groupName={service.group_name} /></dd>
        </div>
      {:else}
        <div class="detail-row">
          <dt>{t('Target')}</dt>
          <dd>{t('Purely mocked service (no target)')}</dd>
        </div>
      {/if}
      <div class="detail-row">
        <dt>{t('Rules')}</dt>
        <dd>{service.rules?.length ?? 0}</dd>
      </div>
    </dl>
  </div>
  <div class="card-actions">
    <button
      type="button"
      class="btn btn-sm btn-primary"
      onclick={() => onSelect(service.name, service.group_name)}
      aria-label={t('Configure the service {0}', service.name)}
      data-testid="service-card-configure-button-{service.name}"
    >
      {t('Configure')}
    </button>
    <button
      type="button"
      class="btn btn-sm btn-outline"
      onclick={() => onClone(service)}
      aria-label={t('Duplicate the service {0}', service.name)}
      title={t('Duplicate')}
      data-testid="service-card-clone-button-{service.name}"
    >
      &#10697;
    </button>
  </div>
</article>

<style>
  .service-card {
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-left: var(--line-stem) solid var(--color-proxy);
    border-radius: var(--radius-m);
    padding: var(--space-4) var(--space-5);
  }

  /* The stem tells the mode down a long list: dashed for a mocked service, solid for one relayed to its target. */
  .service-card.mocked {
    border-left-style: dashed;
    border-left-color: var(--color-mock);
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .card-info {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .card-title {
    margin: 0;
    font-size: var(--text-xl);
    font-weight: var(--weight-strong);
  }

  .card-details {
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: var(--line-thin) solid var(--color-border);
  }

  dl {
    margin: 0;
  }

  .detail-row {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-0-5);
    font-size: var(--text-m);
  }

  dt {
    font-weight: var(--weight-medium);
    color: var(--color-text-muted);
    min-width: 4rem;
  }
  dd {
    margin: 0;
  }

  code {
    font-size: var(--text-s);
    background: var(--color-bg);
    padding: var(--space-0-5) var(--space-1-5);
    border-radius: var(--radius-s);
  }

  .card-actions {
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: var(--line-thin) solid var(--color-border);
  }
</style>
