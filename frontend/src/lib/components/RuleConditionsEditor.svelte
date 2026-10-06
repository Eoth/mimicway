<script>
  // The two condition groups of a rule (AND, OR): the conditions already added, and the form that adds or edits one
  // (ConditionForm). `allOf` and `anyOf` belong to the parent, which saves them and sends them to the conflict check:
  // this component asks for changes through callbacks and never keeps a copy of the arrays.
  import ConditionForm from './ConditionForm.svelte';
  import { t } from '../i18n.svelte.js';

  let {
    allOf = [],
    anyOf = [],
    availablePathParams = [],
    queryParamSuggestions = [],
    onAllOfChange = () => {},
    onAnyOfChange = () => {},
  } = $props();

  let addingConditionTo = $state(null);
  // The condition edited in place: { group, idx } or null. Exclusive with addingConditionTo, so that one small form is
  // open at a time: opening one closes the other, and drops what it had not saved, as its Cancel button would. Each
  // opening mounts ConditionForm again, filled from the saved condition.
  let editingCondition = $state(null);

  function addCondition(group, condition) {
    if (group === 'all_of') onAllOfChange([...allOf, condition]);
    else onAnyOfChange([...anyOf, condition]);
    addingConditionTo = null;
  }

  function removeCondition(group, idx) {
    const list = group === 'all_of' ? allOf : anyOf;
    const updated = list.filter((_, i) => i !== idx);
    if (group === 'all_of') onAllOfChange(updated);
    else onAnyOfChange(updated);
    if (editingCondition?.group === group) {
      if (editingCondition.idx === idx) editingCondition = null;
      else if (editingCondition.idx > idx) editingCondition = { group, idx: editingCondition.idx - 1 };
    }
  }

  function startAdd(group) {
    editingCondition = null;
    addingConditionTo = group;
  }

  function startEdit(group, idx) {
    addingConditionTo = null;
    editingCondition = { group, idx };
  }

  function cancelEdit() {
    editingCondition = null;
  }

  // Replaces the entry at `idx` only (map, not filter and push): the other conditions of the group keep their place.
  function saveEdit(condition) {
    const { group, idx } = editingCondition;
    const list = group === 'all_of' ? allOf : anyOf;
    const updated = list.map((c, i) => (i === idx ? condition : c));
    if (group === 'all_of') onAllOfChange(updated);
    else onAnyOfChange(updated);
    editingCondition = null;
  }

  function conditionLabel(c) {
    const src = c.source.type === 'BodyRaw' ? t('Raw body') : `${c.source.type}(${c.source.key})`;
    const op = c.operator.type === 'Exists' ? t('exists') : `${c.operator.type}(${c.operator.value})`;
    return `${src} ${op}`;
  }
</script>

<fieldset class="section" data-testid="rule-form-conditions-allof">
  <legend>{t('AND conditions (all must match)')}</legend>
  <p class="section-help">{t('Without any condition, the rule matches every request.')}</p>
  {#if allOf.length > 0}
    <ul class="cond-list" role="list">
      {#each allOf as cond, idx}
        {#if editingCondition?.group === 'all_of' && editingCondition?.idx === idx}
          <li class="cond-item-editing">
            <ConditionForm
              condition={cond}
              {availablePathParams}
              {queryParamSuggestions}
              onSave={saveEdit}
              onCancel={cancelEdit}
            />
          </li>
        {:else}
          <li class="cond-item">
            <button
              type="button"
              class="cond-label-button"
              onclick={() => startEdit('all_of', idx)}
              aria-label={t('Edit the condition: {0}', conditionLabel(cond))}
              data-testid="rule-form-edit-condition-allof-button-{idx}"
            >
              <span translate="no">{conditionLabel(cond)}</span>
            </button>
            <button
              type="button"
              class="btn-icon btn-icon-s btn-delete"
              onclick={() => removeCondition('all_of', idx)}
              aria-label={t('Delete')}
              data-testid="rule-form-remove-condition-allof-button-{idx}">&#10005;</button
            >
          </li>
        {/if}
      {/each}
    </ul>
  {/if}
  {#if addingConditionTo === 'all_of'}
    <ConditionForm
      {availablePathParams}
      {queryParamSuggestions}
      onSave={(c) => addCondition('all_of', c)}
      onCancel={() => (addingConditionTo = null)}
    />
  {:else}
    <button
      type="button"
      class="btn btn-sm btn-outline"
      onclick={() => startAdd('all_of')}
      data-testid="rule-form-add-condition-allof-button">{t('+ AND condition')}</button
    >
  {/if}
</fieldset>

<fieldset class="section" data-testid="rule-form-conditions-anyof">
  <legend>{t('OR conditions (at least one must match)')}</legend>
  {#if anyOf.length > 0}
    <ul class="cond-list" role="list">
      {#each anyOf as cond, idx}
        {#if editingCondition?.group === 'any_of' && editingCondition?.idx === idx}
          <li class="cond-item-editing">
            <ConditionForm
              condition={cond}
              {availablePathParams}
              {queryParamSuggestions}
              onSave={saveEdit}
              onCancel={cancelEdit}
            />
          </li>
        {:else}
          <li class="cond-item">
            <button
              type="button"
              class="cond-label-button"
              onclick={() => startEdit('any_of', idx)}
              aria-label={t('Edit the condition: {0}', conditionLabel(cond))}
              data-testid="rule-form-edit-condition-anyof-button-{idx}"
            >
              <span translate="no">{conditionLabel(cond)}</span>
            </button>
            <button
              type="button"
              class="btn-icon btn-icon-s btn-delete"
              onclick={() => removeCondition('any_of', idx)}
              aria-label={t('Delete')}
              data-testid="rule-form-remove-condition-anyof-button-{idx}">&#10005;</button
            >
          </li>
        {/if}
      {/each}
    </ul>
  {/if}
  {#if addingConditionTo === 'any_of'}
    <ConditionForm
      {availablePathParams}
      {queryParamSuggestions}
      onSave={(c) => addCondition('any_of', c)}
      onCancel={() => (addingConditionTo = null)}
    />
  {:else}
    <button
      type="button"
      class="btn btn-sm btn-outline"
      onclick={() => startAdd('any_of')}
      data-testid="rule-form-add-condition-anyof-button">{t('+ OR condition')}</button
    >
  {/if}
</fieldset>

<style>
  .cond-list {
    list-style: none;
    padding: 0;
    margin: 0 0 var(--space-2);
  }
  .cond-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1-5) var(--space-2);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    margin-bottom: var(--space-1);
    background: var(--color-bg);
    font-size: var(--text-m);
  }
  .cond-item-editing {
    margin-bottom: var(--space-1);
  }

  .cond-label-button {
    flex: 1;
    min-width: 0;
    text-align: left;
    background: none;
    border: none;
    padding: var(--space-1) var(--space-1-5);
    margin: calc(-1 * var(--space-1)) calc(-1 * var(--space-1-5));
    border-radius: var(--radius-m);
    font: inherit;
    color: inherit;
    cursor: pointer;
    overflow-wrap: anywhere;
  }
  .cond-label-button:hover {
    background: var(--color-surface);
    text-decoration: underline;
  }
</style>
