<script>
  import { t } from '../i18n.svelte.js';
  // A list of items, each with a remove button. `getKey` and `getLabel` adapt it to plain strings or to objects (the
  // members of a group, for instance).
  let {
    items = [],
    getKey = (item) => item,
    getLabel = (item) => String(item),
    onRemove = () => {},
    emptyText = null,
  } = $props();
</script>

{#if items.length === 0}
  <p class="removable-list-empty">{emptyText ?? t('No item.')}</p>
{:else}
  <ul class="removable-list">
    {#each items as item (getKey(item))}
      <li class="removable-list-item" data-testid="removable-list-item-{getKey(item)}">
        <span>{getLabel(item)}</span>
        <button
          type="button"
          class="chip-remove"
          onclick={() => onRemove(item)}
          aria-label={t('Remove {0}', getLabel(item))}
          title={t('Remove')}
          data-testid="removable-list-remove-button-{getKey(item)}"
        >
          &times;
        </button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .removable-list {
    list-style: none;
    padding: 0;
    margin: 0 0 var(--space-2);
  }
  .removable-list-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1) 0;
    font-size: var(--text-m);
  }
  .removable-list-empty {
    color: var(--color-text-muted);
    font-size: var(--text-s);
    margin: var(--space-2) 0;
    font-style: italic;
  }
  .chip-remove {
    background: none;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-weight: var(--weight-heavy);
    font-size: var(--text-m);
    padding: 0 var(--space-1);
    line-height: var(--leading-none);
  }
  .chip-remove:hover {
    color: var(--color-danger);
  }
</style>
