<script>
  import ServiceForm from './ServiceForm.svelte';
  import RuleList from './RuleList.svelte';
  import RuleForm from './RuleForm.svelte';
  import UrlHealthBadge from './UrlHealthBadge.svelte';
  import ObservationSuggestions from './ObservationSuggestions.svelte';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import { updateService, deleteService, reorderRules } from '../api.js';
  import { buildServiceTestUrl } from '../service-url.js';
  import { t } from '../i18n.svelte.js';

  let {
    service,
    availableGroups = [],
    onBack = () => {},
    onUpdate = () => {},
    onDelete = () => {},
    onNotify = () => {},
  } = $props();

  // The URL to call, as the service form shows it, where the rules of the service are written.
  let testUrl = $derived(
    service
      ? buildServiceTestUrl({
          name: service.name,
          listenPath: service.listen_path,
          groupCode: availableGroups.find((g) => g.name === service.group_name)?.code ?? '',
          baseUrl: typeof window !== 'undefined' ? window.location.origin : '',
        })
      : '',
  );
  let editing = $state(false);
  let editingRuleIdx = $state(null);
  let addingRule = $state(false);
  let confirmDelete = $state(false);

  // The handlers read the service's name and group before their first await: `service` is a reactive prop, and
  // onDelete() or onUpdate() can remove the service from the parent (App.svelte) while a request is in flight, which
  // turns `service` null. Values captured up front remove the race; an `if (!service)` guard would only hide it.
  async function handleSaveService(updated) {
    const name = service.name;
    const groupName = service.group_name;
    try {
      const result = await updateService(name, groupName, updated);
      onUpdate(result, groupName);
      editing = false;
      onNotify(t('Service "{0}" updated', result.name), 'success');
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }

  async function handleDeleteService() {
    confirmDelete = false;
    const name = service.name;
    const groupName = service.group_name;
    try {
      await deleteService(name, groupName);
      onNotify(t('Service "{0}" deleted', name), 'success');
      onDelete(name, groupName);
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }

  async function handleReorder(order) {
    const name = service.name;
    const groupName = service.group_name;
    try {
      const result = await reorderRules(name, groupName, order);
      onUpdate(result, groupName);
    } catch (e) {
      onNotify(t('Reordering error: {0}', e.message), 'error');
    }
  }

  async function handleSaveRule(rule) {
    const rules = [...(service.rules || [])];
    if (editingRuleIdx !== null) {
      rules[editingRuleIdx] = rule;
    } else {
      rules.push(rule);
    }
    const updated = { ...service, rules };
    const name = service.name;
    const groupName = service.group_name;
    try {
      const result = await updateService(name, groupName, updated);
      onUpdate(result, groupName);
      editingRuleIdx = null;
      addingRule = false;
      onNotify(t('Rule "{0}" saved', rule.name), 'success');
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }

  function handleCloneRule(idx) {
    const source = JSON.parse(JSON.stringify(service.rules[idx]));
    source.name = '';
    editingRuleIdx = null;
    addingRule = true;
    clonedRule = source;
  }

  // Same flow as handleCloneRule: the creation form opens filled with the suggested draft, and the user reviews, edits
  // and saves it like any other rule (same validation, same conflict check).
  function handleUseSuggestion(ruleDraft) {
    editingRuleIdx = null;
    addingRule = true;
    clonedRule = ruleDraft;
  }

  let clonedRule = $state(null);

  async function handleDeleteRule(idx) {
    const rules = service.rules.filter((_, i) => i !== idx);
    const updated = { ...service, rules };
    const name = service.name;
    const groupName = service.group_name;
    try {
      const result = await updateService(name, groupName, updated);
      onUpdate(result, groupName);
      onNotify(t('Rule deleted'), 'success');
    } catch (e) {
      onNotify(t('Error: {0}', e.message), 'error');
    }
  }
</script>

{#if !service}
  <p>{t('Loading...')}</p>
{:else}
  <div class="service-detail">
    <nav class="detail-nav" aria-label={t('Service navigation')}>
      <button
        type="button"
        class="btn btn-secondary btn-back"
        onclick={onBack}
        data-testid="service-detail-back-button"
      >
        &#8592; {t('Back')}
      </button>
      <h2>{service.name}</h2>
    </nav>

    {#if editing}
      <ServiceForm
        {service}
        {availableGroups}
        isEdit={true}
        onSave={handleSaveService}
        onCancel={() => (editing = false)}
      />
    {:else}
      <div class="detail-card">
        <dl class="detail-dl">
          <div class="dl-row">
            <dt>{t('Listen path')}</dt>
            <dd><code>{service.listen_path}</code></dd>
          </div>
          <div class="dl-row">
            <dt>{t('Test URL')}</dt>
            <dd><code data-testid="service-detail-test-url">{testUrl}</code></dd>
          </div>
          {#if service.real_target_url?.trim()}
            <div class="dl-row">
              <dt>{t('Real target URL')}</dt>
              <dd><code>{service.real_target_url}</code></dd>
            </div>
            <div class="dl-row">
              <dt>{t('Availability')}</dt>
              <dd><UrlHealthBadge serviceName={service.name} groupName={service.group_name} /></dd>
            </div>
          {:else}
            <div class="dl-row">
              <dt>{t('Real target URL')}</dt>
              <dd>{t('Purely mocked service (no target)')}</dd>
            </div>
          {/if}
          <div class="dl-row">
            <dt>{t('Directory URL rewriting')}</dt>
            <dd>{service.rewrite_directory_urls ? t('Yes') : t('No')}</dd>
          </div>
        </dl>
        <div class="detail-actions">
          <button
            type="button"
            class="btn btn-primary"
            onclick={() => (editing = true)}
            data-testid="service-detail-edit-button"
          >
            {t('Edit the service')}
          </button>
          <button
            type="button"
            class="btn btn-danger"
            onclick={() => (confirmDelete = true)}
            data-testid="service-detail-delete-button"
          >
            {t('Delete')}
          </button>
        </div>
      </div>
    {/if}

    <ConfirmDialog
      open={confirmDelete}
      title={t('Delete the service')}
      message={t('Delete the service "{0}"? This cannot be undone.', service.name)}
      confirmLabel={t('Yes, delete')}
      onConfirm={handleDeleteService}
      onCancel={() => (confirmDelete = false)}
    />

    {#if editingRuleIdx !== null}
      <RuleForm
        rule={service.rules[editingRuleIdx]}
        existingRules={(service.rules ?? []).filter((_, i) => i !== editingRuleIdx)}
        draftPosition={editingRuleIdx}
        serviceName={service.name}
        groupName={service.group_name}
        listenPath={service.listen_path}
        isPurelyMocked={!service.real_target_url?.trim()}
        onSave={handleSaveRule}
        onCancel={() => (editingRuleIdx = null)}
      />
    {:else if addingRule}
      <RuleForm
        rule={clonedRule}
        existingRules={service.rules ?? []}
        draftPosition={(service.rules ?? []).length}
        serviceName={service.name}
        groupName={service.group_name}
        listenPath={service.listen_path}
        isPurelyMocked={!service.real_target_url?.trim()}
        onSave={handleSaveRule}
        onCancel={() => {
          addingRule = false;
          clonedRule = null;
        }}
      />
    {:else}
      <RuleList
        rules={service.rules ?? []}
        onReorder={handleReorder}
        onEditRule={(idx) => (editingRuleIdx = idx)}
        onDeleteRule={handleDeleteRule}
        onCloneRule={handleCloneRule}
        onAddRule={() => {
          addingRule = true;
          clonedRule = null;
        }}
      />
      <ObservationSuggestions
        serviceName={service.name}
        groupName={service.group_name}
        isMocked={service.is_mocked}
        onUseSuggestion={handleUseSuggestion}
      />
    {/if}
  </div>
{/if}

<style>
  .service-detail {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .detail-nav {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .detail-nav h2 {
    margin: 0;
  }

  .detail-card {
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-5);
  }

  .detail-dl {
    margin: 0;
  }
  /* On a narrow screen a value goes under its label, and a long URL breaks rather than widening the page. */
  .dl-row {
    display: flex;
    flex-wrap: wrap;
    column-gap: var(--space-2);
    margin-bottom: var(--space-1-5);
  }
  dt {
    font-weight: var(--weight-medium);
    color: var(--color-text-muted);
    min-width: 10rem;
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  code {
    font-size: var(--text-m);
    background: var(--color-bg);
    padding: var(--space-0-5) var(--space-1-5);
    border-radius: var(--radius-s);
  }

  .detail-actions {
    display: flex;
    gap: var(--space-3);
    align-items: center;
    margin-top: var(--space-4);
    padding-top: var(--space-4);
    border-top: var(--line-thin) solid var(--color-border);
  }

  .btn-back {
    padding: var(--space-1-5) var(--space-3);
    font-size: var(--text-m);
  }
</style>
