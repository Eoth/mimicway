<script>
  import { t, tCount } from '../i18n.svelte.js';
  let {
    rules = [],
    onReorder = () => {},
    onEditRule = () => {},
    onDeleteRule = () => {},
    onCloneRule = () => {},
    onAddRule = () => {},
  } = $props();

  let dragIdx = $state(null);
  let dragOverIdx = $state(null);

  function handleDragStart(e, idx) {
    dragIdx = idx;
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', String(idx));
  }

  function handleDragOver(e, idx) {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    dragOverIdx = idx;
  }

  function handleDragLeave() {
    dragOverIdx = null;
  }

  function handleDrop(e, targetIdx) {
    e.preventDefault();
    dragOverIdx = null;
    if (dragIdx === null || dragIdx === targetIdx) return;
    reorder(dragIdx, targetIdx);
    dragIdx = null;
  }

  function handleDragEnd() {
    dragIdx = null;
    dragOverIdx = null;
  }

  function moveUp(idx) {
    if (idx <= 0) return;
    reorder(idx, idx - 1);
  }

  function moveDown(idx) {
    if (idx >= rules.length - 1) return;
    reorder(idx, idx + 1);
  }

  function reorder(fromIdx, toIdx) {
    const reordered = [...rules];
    const [item] = reordered.splice(fromIdx, 1);
    reordered.splice(toIdx, 0, item);
    onReorder(reordered.map((r) => r.name));
  }

  function clickEdit(e, idx) {
    e.stopPropagation();
    e.preventDefault();
    onEditRule(idx);
  }

  function clickDelete(e, idx) {
    e.stopPropagation();
    e.preventDefault();
    onDeleteRule(idx);
  }

  function clickAdd(e) {
    e.stopPropagation();
    e.preventDefault();
    onAddRule();
  }
</script>

<section class="rule-list-section" aria-label={t('Rule list')}>
  <div class="rule-list-header">
    <h3>{t('Matching rules')}</h3>
    <button type="button" class="btn btn-sm btn-primary" onclick={clickAdd} data-testid="rule-list-add-button">
      {t('+ Add a rule')}
    </button>
  </div>

  {#if rules.length === 0}
    <p class="empty-rules" role="status" data-testid="rule-list-empty-message">
      {t('No rule defined. Requests will get 404.')}
    </p>
  {:else}
    <p class="rule-hint" id="rule-order-hint">
      {t('The first matching rule wins. Reorder by drag and drop or with the buttons.')}
    </p>
    <ol class="rule-list" aria-describedby="rule-order-hint" role="list">
      {#each rules as rule, idx (rule.name)}
        <li
          class="rule-item"
          class:proxied={rule.action === 'proxy'}
          class:dragging={dragIdx === idx}
          class:drag-over={dragOverIdx === idx}
          data-testid="rule-list-item-{rule.name}"
        >
          <div
            class="rule-grip"
            aria-hidden="true"
            draggable="true"
            ondragstart={(e) => handleDragStart(e, idx)}
            ondragover={(e) => handleDragOver(e, idx)}
            ondragleave={handleDragLeave}
            ondrop={(e) => handleDrop(e, idx)}
            ondragend={handleDragEnd}
            role="button"
            tabindex="-1"
            data-testid="rule-list-grip-{rule.name}"
          >
            &#9776;
          </div>

          <div class="rule-content">
            <span class="rule-index" aria-hidden="true">{idx + 1}</span>
            <span class="method-badge" data-method={rule.method}>{rule.method}</span>
            <span class="badge rule-action-badge {rule.action === 'proxy' ? 'badge-proxy' : 'badge-mock'}"
              >{rule.action === 'proxy' ? t('PROXY') : t('MOCK')}</span
            >
            <span class="rule-name">{rule.name}</span>
            <span class="rule-meta">
              {#if rule.conditions?.all_of?.length || rule.conditions?.any_of?.length}
                {tCount(
                  (rule.conditions?.all_of?.length ?? 0) + (rule.conditions?.any_of?.length ?? 0),
                  '{0} condition',
                  '{0} conditions',
                )}
              {:else}
                {t('Catch-all')}
              {/if}
            </span>
          </div>

          <div class="rule-actions">
            <button
              type="button"
              class="btn-icon"
              onclick={() => moveUp(idx)}
              disabled={idx === 0}
              aria-label={t('Move the rule {0} up', rule.name)}
              title={t('Move up')}
              data-testid="rule-list-moveup-button-{rule.name}">&#9650;</button
            >
            <button
              type="button"
              class="btn-icon"
              onclick={() => moveDown(idx)}
              disabled={idx === rules.length - 1}
              aria-label={t('Move the rule {0} down', rule.name)}
              title={t('Move down')}
              data-testid="rule-list-movedown-button-{rule.name}">&#9660;</button
            >
            <button
              type="button"
              class="btn-icon btn-edit"
              onclick={(e) => clickEdit(e, idx)}
              aria-label={t('Edit the rule {0}', rule.name)}
              title={t('Edit')}
              data-testid="rule-list-edit-button-{rule.name}">&#9998;</button
            >
            <button
              type="button"
              class="btn-icon"
              onclick={(e) => {
                e.stopPropagation();
                onCloneRule(idx);
              }}
              aria-label={t('Duplicate the rule {0}', rule.name)}
              title={t('Duplicate')}
              data-testid="rule-list-clone-button-{rule.name}">&#10697;</button
            >
            <button
              type="button"
              class="btn-icon btn-delete"
              onclick={(e) => clickDelete(e, idx)}
              aria-label={t('Delete the rule {0}', rule.name)}
              title={t('Delete')}
              data-testid="rule-list-delete-button-{rule.name}">&#10005;</button
            >
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .rule-list-section {
    margin-top: var(--space-5);
  }
  .rule-list-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-2);
  }
  .rule-list-header h3 {
    margin: 0;
    font-size: var(--text-l);
  }
  .rule-hint {
    font-size: var(--text-s);
    color: var(--color-text-muted);
    margin: 0 0 var(--space-2);
  }
  .empty-rules {
    color: var(--color-text-muted);
    font-style: italic;
    padding: var(--space-4);
    text-align: center;
    background: var(--color-bg);
    border-radius: var(--radius-m);
  }

  .rule-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1-5);
  }

  .rule-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-left: var(--line-stem) dashed var(--color-mock);
    border-radius: var(--radius-m);
    transition:
      box-shadow var(--duration-quick),
      border-color var(--duration-quick);
  }

  .rule-item.dragging {
    opacity: 0.4;
  }
  .rule-item.proxied {
    border-left-style: solid;
    border-left-color: var(--color-proxy);
  }
  .rule-item.drag-over {
    border-color: var(--color-primary);
    box-shadow: 0 0 0 var(--line-thin) var(--color-primary);
  }

  .rule-grip {
    color: var(--color-text-muted);
    font-size: var(--text-m);
    flex-shrink: 0;
    user-select: none;
    cursor: grab;
    padding: var(--space-1);
  }

  .rule-grip:active {
    cursor: grabbing;
  }

  .rule-content {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .rule-index {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.5rem;
    height: 1.5rem;
    border-radius: var(--radius-round);
    background: var(--color-bg);
    font-size: var(--text-xs);
    font-weight: var(--weight-heavy);
    flex-shrink: 0;
  }
  .rule-action-badge {
    flex-shrink: 0;
  }
  .rule-name {
    font-weight: var(--weight-strong);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rule-meta {
    font-size: var(--text-s);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .rule-actions {
    display: flex;
    gap: var(--space-1);
    flex-shrink: 0;
  }
</style>
