<script>
  // Observation of the traffic of a pure proxy service (is_mocked=false, src/server/observation.rs), and rule
  // suggestions computed from what it captured. Nothing is automatic: the user turns observation on and off and
  // refreshes the suggestions (no background polling), so the cost follows actual use, as for UrlHealthBadge.svelte.
  //
  // "Use this suggestion" does not create the rule: it fills the rule form (RuleForm.svelte, through onUseSuggestion
  // in ServiceDetail.svelte), so that the user reviews it and saves it through the usual checks (validation on the
  // server, conflict check), rather than through a one-click endpoint that would skip them.
  //
  // A suggestion stands for a route: the server groups the observed paths under a pattern (`/orders/{id}`), listed with
  // a few of the paths it covers, so the user sees what the pattern was inferred from.
  //
  // The backends named in the responses (a directory's addresses, links, redirects) come with them: the application
  // calls those directly, past Mimicway. "Create a proxy service in front of it" opens the usual service form filled
  // (onCreateService, App.svelte), never a one-click creation; once that service exists, the server writes the
  // suggested rules with its address in Mimicway instead of the backend's.
  import {
    observeService,
    unobserveService,
    getObservationStatus,
    getServiceSuggestions,
    getLinkedBackends,
  } from '../api.js';
  import { t, tCount } from '../i18n.svelte.js';

  let {
    serviceName,
    groupName = null,
    isMocked = true,
    onUseSuggestion = () => {},
    onCreateService = () => {},
  } = $props();

  let observing = $state(false);
  let statusLoaded = $state(false);
  let toggling = $state(false);
  let suggestions = $state([]);
  let linkedBackends = $state([]);
  let loadingSuggestions = $state(false);
  let error = $state('');

  async function refreshStatus() {
    if (isMocked) return;
    try {
      const active = await getObservationStatus();
      observing = active.some((e) => e.service_name === serviceName && (e.group_name ?? null) === (groupName ?? null));
    } catch {
      // Not critical: keep the last known status rather than block the panel on a passing network error.
    } finally {
      statusLoaded = true;
    }
  }
  $effect(() => {
    refreshStatus();
  });

  async function handleToggleObserve() {
    toggling = true;
    error = '';
    try {
      if (observing) {
        await unobserveService(serviceName, groupName);
        observing = false;
        suggestions = [];
        linkedBackends = [];
      } else {
        await observeService(serviceName, groupName);
        observing = true;
      }
    } catch (e) {
      error = e.message;
    } finally {
      toggling = false;
    }
  }

  async function refreshSuggestions() {
    loadingSuggestions = true;
    error = '';
    try {
      [suggestions, linkedBackends] = await Promise.all([
        getServiceSuggestions(serviceName, groupName),
        getLinkedBackends(serviceName, groupName),
      ]);
    } catch (e) {
      error = e.message;
    } finally {
      loadingSuggestions = false;
    }
  }

  // A pure proxy in front of the backend, in the observed service's group: once saved it can be observed in turn.
  function toServiceDraft(draft) {
    return {
      name: draft.name,
      listen_path: '',
      real_target_url: draft.real_target_url,
      is_mocked: false,
      group_name: draft.group_name ?? null,
      wsdl_mode: 'auto',
      rules: [],
    };
  }

  function serviceNames(refs) {
    return (refs ?? []).map((r) => r.name).join(', ');
  }

  function sourceLabel(type) {
    switch (type) {
      case 'QueryParam':
        return t('Query parameter');
      case 'Header':
        return t('HTTP header');
      case 'JsonPointer':
        return t('JSON field of the body');
      case 'PathParam':
        return t('Path parameter');
      default:
        return type;
    }
  }

  function conditionSummary(condition) {
    if (!condition) return null;
    return t(
      '{0} "{1}" = "{2}"',
      sourceLabel(condition.source?.type),
      condition.source?.key,
      condition.operator?.value,
    );
  }

  function toRuleDraft(rule) {
    return {
      name: '',
      method: rule.method,
      sub_path: rule.sub_path,
      action: 'mock',
      script: null,
      response_mode: null,
      conditions: {
        all_of: rule.condition ? [rule.condition] : [],
        any_of: [],
      },
      response: rule.response,
    };
  }

  // An echo rule's body alternates literal text and the expression that repeats the request value: both are shown,
  // so that the preview reads as the template it is.
  function bodyPreview(rule) {
    const text = (rule.response?.body ?? [])
      .map((f) => (f.type === 'Literal' ? f.value : f.type === 'Template' ? f.template : ''))
      .join('');
    return text.length > 120 ? `${text.slice(0, 120)}…` : text;
  }

  function rulesOf(suggestion) {
    return suggestion.outcome === 'Conditional' ? suggestion.rules : [suggestion.rule];
  }

  function ruleSummary(suggestion, rule) {
    if (rule.condition) return t('if {0}', conditionSummary(rule.condition));
    if (suggestion.outcome === 'Echo') {
      return t(
        'no condition: the response repeats {0} "{1}" ({2} calls)',
        sourceLabel(rule.echo?.type),
        rule.echo?.key,
        rule.sample_count,
      );
    }
    return t('no condition ({0} identical calls)', rule.sample_count);
  }
</script>

{#if !isMocked}
  <section
    class="observation-panel"
    aria-label={t('Observation of the proxied traffic')}
    data-testid="observation-panel-{serviceName}"
  >
    <div class="panel-header">
      <h4>{t('Rule suggestions from real traffic')}</h4>
      <button
        type="button"
        class="btn btn-sm {observing ? 'btn-outline' : 'btn-primary'}"
        onclick={handleToggleObserve}
        disabled={toggling || !statusLoaded}
        data-testid="observation-toggle-button-{serviceName}"
      >
        {observing ? t('Stop observing') : t('Observe this service')}
      </button>
    </div>
    <p class="panel-hint">
      {t(
        'Captures bounded copies of requests and responses while this service is a pure proxy, to suggest mock rules from calls actually observed. Never switched on automatically.',
      )}
    </p>

    {#if observing}
      <div class="panel-actions">
        <button
          type="button"
          class="btn btn-sm btn-outline"
          onclick={refreshSuggestions}
          disabled={loadingSuggestions}
          data-testid="observation-refresh-suggestions-button-{serviceName}"
        >
          {loadingSuggestions ? t('Loading...') : t('Refresh the suggestions')}
        </button>
      </div>

      {#if suggestions.length === 0 && !loadingSuggestions}
        <p class="panel-empty" data-testid="observation-suggestions-empty-{serviceName}">
          {t('No suggestion yet: call this service through the proxy several times, then refresh.')}
        </p>
      {/if}

      {#each suggestions as suggestion, i (i)}
        {#if suggestion.path_count > 1 || suggestion.paths[0] !== suggestion.sub_path}
          <p class="suggestion-paths" data-testid="observation-suggestion-paths-{serviceName}-{i}">
            {tCount(
              suggestion.path_count,
              '{1} {2} stands for the observed path {3}',
              '{1} {2} groups {0} observed paths, such as {3}',
              suggestion.method,
              suggestion.sub_path,
              suggestion.paths.slice(0, 3).join(', '),
            )}
          </p>
        {/if}
        {#if suggestion.outcome === 'VarianceUnexplained'}
          <p class="panel-unexplained" role="status" data-testid="observation-suggestion-unexplained-{serviceName}-{i}">
            <code>{suggestion.method} {suggestion.sub_path}</code>
            {t(
              'Varying responses observed ({0} calls, {1} distinct responses) but no field of the request tells them apart reliably: no rule suggested.',
              suggestion.sample_count,
              suggestion.response_class_count,
            )}
          </p>
        {:else}
          {#each rulesOf(suggestion) as rule, j (j)}
            <div class="suggestion-card" data-testid="observation-suggestion-{serviceName}-{i}-{j}">
              <div class="suggestion-summary">
                <code>{rule.method} {rule.sub_path}</code>
                <span class="suggestion-condition">{ruleSummary(suggestion, rule)}</span>
              </div>
              <div class="suggestion-response">
                <span class="suggestion-status">{rule.response.status}</span>
                <code class="suggestion-body">{bodyPreview(rule)}</code>
              </div>
              {#if rule.redirected_to?.length}
                <p class="suggestion-redirect" data-testid="observation-suggestion-redirect-{serviceName}-{i}-{j}">
                  {tCount(
                    rule.redirected_to.length,
                    'Sends the application to the service {1} in Mimicway, instead of the backend behind it.',
                    'Sends the application to the services {1} in Mimicway, instead of the backends behind them.',
                    serviceNames(rule.redirected_to),
                  )}
                </p>
              {/if}
              <button
                type="button"
                class="btn btn-sm btn-primary"
                onclick={() => onUseSuggestion(toRuleDraft(rule))}
                data-testid="observation-use-suggestion-{serviceName}-{i}-{j}"
              >
                {t('Use this suggestion')}
              </button>
            </div>
          {/each}
        {/if}
      {/each}

      {#if linkedBackends.length > 0}
        <div class="linked-backends" data-testid="observation-linked-backends-{serviceName}">
          <h5>{t('Backends named in the responses')}</h5>
          <p class="panel-hint">
            {t(
              'The application calls these addresses directly, past Mimicway. Put a proxy service in front of one, then refresh: the suggested rules send the application to it.',
            )}
          </p>
          <ul>
            {#each linkedBackends as backend, k (backend.origin)}
              <li class="linked-backend" data-testid="observation-linked-backend-{serviceName}-{k}">
                <code>{backend.origin}</code>
                <span class="linked-backend-detail">
                  {t('named by {0}, such as {1}', backend.routes.join(', '), backend.addresses[0])}
                </span>
                {#if backend.served_by.length}
                  <span class="linked-backend-detail">
                    {tCount(
                      backend.served_by.length,
                      'Served by the service {1}.',
                      'Served by the services {1}.',
                      serviceNames(backend.served_by),
                    )}
                  </span>
                {/if}
                {#if backend.draft}
                  <button
                    type="button"
                    class="btn btn-sm btn-outline"
                    onclick={() => onCreateService(toServiceDraft(backend.draft))}
                    data-testid="observation-create-service-{serviceName}-{k}"
                  >
                    {t('Create a proxy service in front of it')}
                  </button>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    {/if}

    {#if error}
      <p class="panel-error" role="alert" data-testid="observation-error-{serviceName}">{error}</p>
    {/if}
  </section>
{/if}

<style>
  .observation-panel {
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-4) var(--space-5);
    margin-top: var(--space-4);
  }

  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .panel-header h4 {
    margin: 0;
    font-size: var(--text-l);
  }

  .panel-hint {
    margin: var(--space-2) 0 0;
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }

  .panel-actions {
    margin-top: var(--space-3);
  }

  .suggestion-paths {
    margin: var(--space-4) 0 0;
    font-size: var(--text-s);
    color: var(--color-text-muted);
    overflow-wrap: anywhere;
  }

  .panel-empty,
  .panel-unexplained {
    margin: var(--space-3) 0 0;
    font-size: var(--text-m);
    color: var(--color-text-muted);
  }

  .suggestion-card {
    margin-top: var(--space-3);
    padding: var(--space-3);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-1-5);
  }

  .suggestion-summary {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .suggestion-condition {
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }

  .suggestion-response {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .suggestion-status {
    font-weight: var(--weight-heavy);
    font-size: var(--text-s);
    padding: var(--space-0-5) var(--space-2);
    border-radius: var(--radius-m);
    background: var(--color-bg);
  }

  .suggestion-body {
    font-size: var(--text-s);
    background: var(--color-bg);
    padding: var(--space-0-5) var(--space-1-5);
    border-radius: var(--radius-s);
    overflow-wrap: anywhere;
  }

  .suggestion-redirect {
    margin: 0;
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }

  .linked-backends {
    margin-top: var(--space-5);
  }

  .linked-backends h5 {
    margin: 0;
    font-size: var(--text-m);
  }

  .linked-backends ul {
    list-style: none;
    margin: var(--space-3) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .linked-backend {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-1);
    overflow-wrap: anywhere;
  }

  .linked-backend-detail {
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }

  code {
    font-size: var(--text-s);
  }

  .panel-error {
    margin: var(--space-3) 0 0;
    font-size: var(--text-s);
    color: var(--color-danger);
  }
</style>
