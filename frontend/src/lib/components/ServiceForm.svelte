<script>
  import { untrack } from 'svelte';
  import FormField from './FormField.svelte';
  import ToggleSwitch from './ToggleSwitch.svelte';
  import { buildServiceTestUrl } from '../service-url.js';
  import { t, tCount } from '../i18n.svelte.js';

  let {
    service = null,
    existingNames = [],
    availableGroups = [],
    isEdit = false,
    onSave = () => {},
    onCancel = () => {},
  } = $props();
  let name = $state(untrack(() => service?.name ?? ''));
  let listenPath = $state(untrack(() => service?.listen_path ?? ''));
  let realTargetUrl = $state(untrack(() => service?.real_target_url ?? 'http://'));
  // "Purely mocked" is not stored: an empty real_target_url is what makes a service purely mocked.
  let purelyMocked = $state(untrack(() => (service ? !service.real_target_url?.trim() : false)));
  // The proxy rules of the service before this edit: checking "Purely mocked" warns about them on save, without
  // blocking.
  const proxyRulesAffected = untrack(() => (service?.rules ?? []).filter((r) => r.action === 'proxy'));
  let pendingPurelyMockedWarning = $state(false);
  let pendingPayload = $state(null);
  let serviceType = $state(
    untrack(() => {
      if (service?.wsdl_mode === 'mock' || service?.wsdl_mode === 'proxy') return 'soap';
      return service?.rewrite_directory_urls ? 'soap' : 'rest';
    }),
  );
  let groupName = $state(untrack(() => service?.group_name ?? ''));

  const RESERVED_NAMES = ['api', 'auth', 'index.html', 'assets', 'favicon.ico'];

  const baseUrl = typeof window !== 'undefined' ? window.location.origin : '';

  let testUrl = $derived(() => {
    const g = availableGroups.find((gr) => gr.name === groupName);
    return buildServiceTestUrl({ name, listenPath, groupCode: g?.code ?? '', baseUrl });
  });
  let saving = $state(false);
  let error = $state('');

  function validateName(n) {
    const trimmed = n.trim();
    if (!trimmed) return t('The service name is required.');
    if (RESERVED_NAMES.includes(trimmed.toLowerCase())) {
      return t('The name "{0}" is reserved by Mimicway (forbidden names: {1}).', trimmed, RESERVED_NAMES.join(', '));
    }
    if (trimmed.includes('/') || trimmed.includes('\\')) {
      return t('A service name cannot contain a path separator (/ or \\).');
    }
    if (!/^[A-Za-z0-9_-]+$/.test(trimmed)) {
      return t('A service name can only contain letters, digits, dashes (-) and underscores (_).');
    }
    return null;
  }

  function validatePath(_p) {
    return null;
  }

  // Unchecking shows the target field again with its previous value: checking the box hides realTargetUrl but never
  // clears it. A field that never held a value starts from 'http://', as for a new service.
  function handlePurelyMockedChange(val) {
    purelyMocked = val;
    if (!val && !realTargetUrl.trim()) {
      realTargetUrl = 'http://';
    }
  }

  function buildPayload() {
    const isSoap = serviceType === 'soap';
    // A purely mocked service is always mocked (the server refuses a pure proxy without a target) and sends an empty
    // real_target_url, whatever the hidden field still holds.
    const payload = {
      name: name.trim(),
      listen_path: listenPath.trim(),
      real_target_url: purelyMocked ? '' : realTargetUrl.trim(),
      is_mocked: purelyMocked ? true : (service?.is_mocked ?? false),
      rewrite_directory_urls: isSoap,
      // The form does not show the WSDL mode: it keeps the one the service has (set through the API or the
      // configuration file), 'auto' for a new service.
      wsdl_mode: service?.wsdl_mode ?? 'auto',
      rules: service?.rules ?? [],
    };
    if (groupName) payload.group_name = groupName;
    return payload;
  }

  async function submitPayload(payload) {
    saving = true;
    try {
      await onSave(payload);
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  async function handleSubmit(e) {
    e.preventDefault();
    error = '';
    pendingPurelyMockedWarning = false;
    pendingPayload = null;

    const nameErr = validateName(name);
    if (nameErr) {
      error = nameErr;
      return;
    }

    const pathErr = validatePath(listenPath);
    if (pathErr) {
      error = pathErr;
      return;
    }

    if (!purelyMocked && !realTargetUrl.trim()) {
      error = t('The target URL is required.');
      return;
    }

    const payload = buildPayload();

    // Warn rather than block, as the rule conflict check does (RuleForm.svelte): the proxy rules keep working until the
    // user confirms.
    if (purelyMocked && proxyRulesAffected.length > 0) {
      pendingPurelyMockedWarning = true;
      pendingPayload = payload;
      return;
    }

    await submitPayload(payload);
  }

  function confirmSaveDespitePurelyMockedWarning() {
    const payload = pendingPayload;
    pendingPurelyMockedWarning = false;
    pendingPayload = null;
    if (payload) submitPayload(payload);
  }

  function cancelPurelyMockedWarning() {
    pendingPurelyMockedWarning = false;
    pendingPayload = null;
  }
</script>

<form
  class="service-form"
  onsubmit={handleSubmit}
  aria-label={isEdit ? t('Edit the service {0}', name) : t('Add a service')}
>
  {#if error}
    <div class="form-error" role="alert" aria-live="assertive" data-testid="service-form-error">{error}</div>
  {/if}

  <FormField id="svc-name" label={t('Service name')} hint={t('Unique identifier, also the URL prefix: /{name}/...')}>
    {#snippet children({ id, describedBy })}
      <input
        {id}
        type="text"
        bind:value={name}
        required
        disabled={isEdit}
        placeholder={t('e.g. users-service')}
        aria-describedby={describedBy}
        data-testid="service-form-name-input"
      />
    {/snippet}
  </FormField>

  <FormField
    id="svc-path"
    label={t('Listen path (optional)')}
    hint={t(
      'Leave empty to intercept all the traffic under /{name}/. Otherwise use /* as a wildcard or {param} to capture segments.',
    )}
  >
    {#snippet children({ id, describedBy })}
      <input
        {id}
        type="text"
        bind:value={listenPath}
        placeholder={t('Empty = intercepts everything under the service name')}
        aria-describedby={describedBy}
        data-testid="service-form-path-input"
      />
    {/snippet}
  </FormField>

  <div class="form-field">
    <ToggleSwitch
      label={t('Purely mocked service')}
      name="purely-mocked"
      checked={purelyMocked}
      onchange={handlePurelyMockedChange}
    />
    <span class="field-hint"
      >{t(
        'No real target: no proxy mode, no availability test. Can be switched on at any time without losing the rules already configured.',
      )}</span
    >
  </div>

  {#if !purelyMocked}
    <FormField
      id="svc-target"
      label={t('Real target URL')}
      hint={t('Address of the real backend (used in proxy mode)')}
    >
      {#snippet children({ id, describedBy })}
        <input
          {id}
          type="url"
          bind:value={realTargetUrl}
          required
          placeholder={t('e.g. http://users-service.default.svc:8080')}
          aria-describedby={describedBy}
          data-testid="service-form-target-input"
        />
      {/snippet}
    </FormField>
  {/if}

  {#if pendingPurelyMockedWarning}
    <div class="callout callout-warning" role="alert" data-testid="service-form-purely-mocked-warning">
      <p>
        {tCount(
          proxyRulesAffected.length,
          '⚠ This rule of the service uses the "Proxy" action and will stop working once the service is purely mocked (it will return a clear error instead of forwarding to a target): {1}.',
          '⚠ These rules of the service use the "Proxy" action and will stop working once the service is purely mocked (they will return a clear error instead of forwarding to a target): {1}.',
          proxyRulesAffected.map((r) => r.name).join(', '),
        )}
      </p>
      <div class="callout-actions">
        <button
          type="button"
          class="btn btn-sm btn-primary"
          onclick={confirmSaveDespitePurelyMockedWarning}
          data-testid="service-form-purely-mocked-save-anyway-button">{t('Save anyway')}</button
        >
        <button
          type="button"
          class="btn btn-sm btn-secondary"
          onclick={cancelPurelyMockedWarning}
          data-testid="service-form-purely-mocked-cancel-button">{t('Go back')}</button
        >
      </div>
    </div>
  {/if}

  <FormField
    id="svc-type"
    label={t('Service type')}
    hint={serviceType === 'soap'
      ? t('?wsdl requests will be forwarded to the real backend automatically.')
      : t('Standard REST API (JSON).')}
  >
    {#snippet children({ id, describedBy })}
      <select {id} bind:value={serviceType} aria-describedby={describedBy} data-testid="service-form-type-select">
        <option value="rest">{t('REST')}</option>
        <option value="soap">{t('SOAP / XML')}</option>
      </select>
    {/snippet}
  </FormField>

  {#if availableGroups.length > 0}
    <FormField id="svc-group" label={t('Group')} hint={t('Puts the service in a group, which manages access rights')}>
      {#snippet children({ id, describedBy })}
        <select {id} bind:value={groupName} aria-describedby={describedBy} data-testid="service-form-group-select">
          <option value="">{t('-- No group --')}</option>
          {#each availableGroups as g}
            <option value={g.name}>{g.name} (/{g.code})</option>
          {/each}
        </select>
      {/snippet}
    </FormField>
  {/if}

  {#if name.trim()}
    <div class="url-preview">
      <strong>{t('Test URL:')}</strong> <code data-testid="service-form-url-preview">{testUrl()}</code>
    </div>
  {/if}

  <div class="form-actions">
    <button type="submit" class="btn btn-primary" disabled={saving} data-testid="service-form-submit-button">
      {saving ? t('Saving...') : isEdit ? t('Save') : t('Add')}
    </button>
    <button
      type="button"
      class="btn btn-secondary"
      onclick={onCancel}
      disabled={saving}
      data-testid="service-form-cancel-button"
    >
      {t('Cancel')}
    </button>
  </div>
</form>

<style>
  .service-form {
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-6);
  }

  .url-preview {
    background: var(--color-bg);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-3) var(--space-3);
    margin-bottom: var(--space-4);
    font-size: var(--text-m);
  }
  .url-preview code {
    background: none;
    padding: 0;
    font-weight: var(--weight-strong);
    color: var(--color-primary);
  }
</style>
