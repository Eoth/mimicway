<script>
  import ServiceCard from './ServiceCard.svelte';
  import { t, tCount } from '../i18n.svelte.js';

  let {
    groupName = null,
    groupId = 'ungrouped',
    groupCode = '',
    services = [],
    expanded = false,
    onToggleGroup = () => {},
    onToggle = () => {},
    onSelect = () => {},
    onClone = () => {},
  } = $props();
</script>

<div class="service-group">
  <button
    type="button"
    class="group-header"
    id="group-header-{groupId}"
    aria-expanded={expanded}
    aria-controls="group-panel-{groupId}"
    onclick={onToggleGroup}
    data-testid="service-group-header-{groupId}"
  >
    <span class="group-chevron" class:expanded aria-hidden="true">&#9654;</span>
    <h3 class="group-name">{groupName ?? t('No group')}</h3>
    <span class="group-count">{tCount(services.length, '{0} service', '{0} services')}</span>
  </button>

  {#if expanded}
    <div
      id="group-panel-{groupId}"
      role="region"
      aria-labelledby="group-header-{groupId}"
      class="group-panel"
      data-testid="service-group-panel-{groupId}"
    >
      <ul class="service-list" role="list">
        {#each services as service (service.name)}
          <li>
            <ServiceCard {service} {groupCode} {onToggle} {onSelect} {onClone} />
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .service-group {
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    background: var(--color-surface);
    overflow: hidden;
  }

  .group-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-3) var(--space-4);
    background: var(--color-bg);
    border: none;
    cursor: pointer;
    text-align: left;
    font: inherit;
    color: var(--color-text);
    transition: background var(--duration-quick);
  }

  .group-header:hover {
    background: var(--color-hover);
  }

  /* Inside the header: the group's box clips what overflows it. */
  .group-header:focus-visible {
    outline-offset: calc(-2 * var(--line-thick));
  }

  .group-chevron {
    font-size: var(--text-xs);
    transition: transform var(--duration-move) ease;
    flex-shrink: 0;
    color: var(--color-text-muted);
  }

  .group-chevron.expanded {
    transform: rotate(90deg);
  }

  .group-name {
    margin: 0;
    font-size: var(--text-m);
    font-weight: var(--weight-heavy);
  }

  .group-count {
    margin-left: auto;
    font-size: var(--text-s);
    color: var(--color-text-muted);
    font-weight: var(--weight-regular);
  }

  .group-panel {
    padding: var(--space-2);
  }

  .service-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
</style>
