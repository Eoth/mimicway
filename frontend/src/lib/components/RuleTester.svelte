<script>
  // Rule tester: replays the draft rule of RuleForm, read-only, against a request already captured in the service's
  // log. Nothing changes and nothing is proxied: one POST to /api/rule-test (stateless, src/server/api.rs) evaluates
  // the method, the sub-path and each condition (MatchEngine::evaluate_rule_test, src/engine/matcher.rs) and, when the
  // rule matches and is not a proxy rule, runs its three script slots against that real request.
  //
  // Why scripts run here: in production a failing script (unknown Rhai function, type error...) is swallowed
  // (run_rule_script in src/server/intercept.rs), so that a broken script never blocks a request; only a server log
  // line records it. The tester is where the user sees that error, before saving, on a real request: an empty, made-up
  // context would report false errors for scripts that rightly read the body or the parameters (the
  // `parse_json(request.body)` pattern of docs/en/rhai-scripts.md).
  //
  // RuleForm passes the logs of the current service; only the entries with captured details are kept here. A pure
  // proxy service streams its requests without buffering them, so they have none.
  import { testRule } from '../api.js';
  import { formatDateTime } from '../format-date.js';
  import FormField from './FormField.svelte';
  import { t, tCount, intlLocale } from '../i18n.svelte.js';

  let { serviceName, groupName = null, logs = [], getDraftRule } = $props();

  let testableLogs = $derived(logs.filter((l) => l.captured));

  let selectedIndex = $state('');
  let testing = $state(false);
  let result = $state(null);
  let errorMessage = $state('');

  function sourceName(type) {
    switch (type) {
      case 'QueryParam':
        return t('query parameter');
      case 'Header':
        return t('header');
      case 'PathParam':
        return t('path parameter');
      case 'JsonPointer':
        return t('JSON Pointer');
      case 'XPath':
        return t('XPath');
      case 'FormField':
        return t('form field');
      case 'BodyRaw':
        return t('raw body');
      default:
        return type;
    }
  }

  function sourceLabel(source) {
    const base = sourceName(source.type);
    return source.type === 'BodyRaw' ? base : `${base} '${source.key}'`;
  }

  function operatorLabel(operator) {
    if (operator.type === 'Exists') return t('exists');
    if (operator.type === 'Eq') return `= '${operator.value}'`;
    if (operator.type === 'Contains') return t("contains '{0}'", operator.value);
    if (operator.type === 'Regex') return t('matches /{0}/', operator.value);
    return operator.type;
  }

  function logLabel(log) {
    const date = formatDateTime(log.timestamp, undefined, intlLocale());
    return `${log.method} ${log.path} — ${log.mode} — ${date}`;
  }

  async function handleTest() {
    if (selectedIndex === '') return;
    const log = testableLogs[Number(selectedIndex)];
    const draft = getDraftRule();
    testing = true;
    errorMessage = '';
    result = null;
    try {
      result = await testRule({
        method: draft.method,
        sub_path: draft.subPath || null,
        conditions: { all_of: draft.allOf, any_of: draft.anyOf },
        action: draft.action ?? 'mock',
        pre_script: draft.preScript ?? null,
        script: draft.script ?? null,
        post_script: draft.postScript ?? null,
        request: {
          method: log.method,
          remaining_path: log.captured.remaining_path,
          path_params: log.captured.path_params,
          query_params: log.captured.query_params,
          headers: log.captured.headers,
          body: log.captured.body,
          body_truncated: log.captured.body_truncated,
          content_type: log.captured.content_type,
        },
      });
    } catch (e) {
      errorMessage = e.message;
    } finally {
      testing = false;
    }
  }

  function bodyBasedSource(type) {
    return type === 'JsonPointer' || type === 'XPath' || type === 'FormField' || type === 'BodyRaw';
  }

  let showBodyTruncationWarning = $derived(
    !!result?.body_truncated &&
      [...(result.all_of ?? []), ...(result.any_of ?? [])].some((e) => bodyBasedSource(e.condition.source.type)),
  );

  function slotLabel(slot) {
    switch (slot) {
      case 'pre_script':
        return t('Pre-script (preparation)');
      case 'script':
        return t('Custom script');
      case 'post_script':
        return t('Post-script (finalization)');
      default:
        return slot;
    }
  }
</script>

<section class="rule-tester" aria-label={t('Rule tester against a real request')}>
  <h3>{t('Test against a real request')}</h3>

  {#if logs.length === 0}
    <p class="section-help">{t('No request has been captured for this service yet.')}</p>
  {:else if testableLogs.length === 0}
    <p class="section-help">
      {t(
        'No request with captured details for this service: requests proxied directly (service not mocked) are not buffered, so no details are available for a test.',
      )}
    </p>
  {:else}
    <FormField
      id="rule-tester-log"
      label={t('Captured request')}
      hint={t('Read-only replay: no request is sent again')}
    >
      {#snippet children({ id, describedBy })}
        <select {id} bind:value={selectedIndex} aria-describedby={describedBy} data-testid="rule-tester-log-select">
          <option value="" disabled>{t('Choose a request')}</option>
          {#each testableLogs as log, idx}
            <option value={String(idx)}>{logLabel(log)}</option>
          {/each}
        </select>
      {/snippet}
    </FormField>

    <button
      type="button"
      class="btn btn-sm btn-secondary"
      disabled={selectedIndex === '' || testing}
      onclick={handleTest}
      data-testid="rule-tester-test-button"
    >
      {testing ? t('Testing…') : t('Test against this request')}
    </button>

    {#if errorMessage}
      <p class="form-error" role="alert" data-testid="rule-tester-error">{errorMessage}</p>
    {/if}

    {#if result}
      <div class="tester-result" role="status" data-testid="rule-tester-result">
        <p class="result-banner" class:result-ok={result.overall_matched} class:result-fail={!result.overall_matched}>
          {#if result.overall_matched}
            {t('✓ This rule would match this request')}
          {:else}
            {t('✗ This rule would not match this request')}
          {/if}
        </p>

        <ul class="result-summary">
          <li>{result.method_matches ? t('✓ HTTP method matches') : t('✗ HTTP method does not match')}</li>
          <li>{result.sub_path_matches ? t('✓ Sub-path matches') : t('✗ Sub-path does not match')}</li>
        </ul>

        {#if result.script_errors?.length > 0}
          <div class="callout callout-danger" role="alert" data-testid="rule-tester-script-errors">
            <p class="callout-title">
              {tCount(
                result.script_errors.length,
                '⚠ A script failed to run: the response would be rendered with an empty result for this script (no error is returned to the client, as in production).',
                '⚠ Scripts failed to run: the response would be rendered with an empty result for these scripts (no error is returned to the client, as in production).',
              )}
            </p>
            <ul class="script-error-list">
              {#each result.script_errors as err}
                <li data-testid="rule-tester-script-error-{err.slot}">
                  <strong>{t('{0}:', slotLabel(err.slot))}</strong> <code>{err.message}</code>
                </li>
              {/each}
            </ul>
          </div>
        {/if}

        {#if result.script_results?.length > 0}
          <div class="script-result-panel" data-testid="rule-tester-script-results">
            <p class="script-result-title">
              {tCount(
                result.script_results.length,
                'Result of this script (no error, but check that these are the expected values):',
                'Result of these scripts (no error, but check that these are the expected values):',
              )}
            </p>
            {#each result.script_results as sr}
              <div class="script-result-slot" data-testid="rule-tester-script-result-{sr.slot}">
                <strong>{slotLabel(sr.slot)}</strong>
                {#if Object.keys(sr.fields).length > 0}
                  <ul class="script-result-fields">
                    {#each Object.entries(sr.fields) as [key, value]}
                      <li>
                        <code>{`{{${sr.slot}.${key}}}`}</code> = <code class="script-result-value">{value}</code>
                      </li>
                    {/each}
                  </ul>
                {:else}
                  <p class="script-result-value-line">
                    <code>{`{{${sr.slot}}}`}</code> = <code class="script-result-value">{sr.value}</code>
                  </p>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if showBodyTruncationWarning}
          <p class="callout callout-warning">
            {t('⚠ The body of this request was truncated in the log: comparisons on the body may be wrong.')}
          </p>
        {/if}

        {#each [['all_of', t('AND conditions'), result.all_of], ['any_of', t('OR conditions'), result.any_of]] as [key, title, evaluations]}
          {#if evaluations.length > 0}
            <div class="condition-group-result">
              <h4>{title}</h4>
              <ul class="condition-eval-list">
                {#each evaluations as ev}
                  <li class="condition-eval" class:eval-ok={ev.matched} class:eval-fail={!ev.matched}>
                    <div class="eval-line">
                      <span class="eval-icon" aria-hidden="true">{ev.matched ? '✓' : '✗'}</span>
                      <span class="eval-text">
                        {ev.matched
                          ? t(
                              '{0} {1}: matches (value found: {2})',
                              sourceLabel(ev.condition.source),
                              operatorLabel(ev.condition.operator),
                              ev.found_value != null ? `'${ev.found_value}'` : t('none'),
                            )
                          : t(
                              '{0} {1}: does not match (value found: {2})',
                              sourceLabel(ev.condition.source),
                              operatorLabel(ev.condition.operator),
                              ev.found_value != null ? `'${ev.found_value}'` : t('none'),
                            )}
                      </span>
                    </div>
                    {#if ev.hint}
                      <p class="eval-hint">{ev.hint}</p>
                    {/if}
                  </li>
                {/each}
              </ul>
            </div>
          {/if}
        {/each}
      </div>
    {/if}
  {/if}
</section>

<style>
  .rule-tester {
    background: var(--color-sunken);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-4);
    margin: var(--space-2) 0 var(--space-4);
  }

  .rule-tester h3 {
    margin: 0 0 var(--space-2);
    font-size: var(--text-l);
  }

  .tester-result {
    margin-top: var(--space-3);
  }

  .result-banner {
    font-weight: var(--weight-strong);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-m);
  }

  .result-ok {
    background: var(--color-success-bg);
    color: var(--color-success-text);
  }

  .result-fail {
    background: var(--color-danger-bg);
    color: var(--color-danger-text);
  }

  .result-summary {
    list-style: none;
    padding: 0;
    margin: var(--space-2) 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-m);
  }

  .script-error-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-s);
    word-break: break-word;
  }

  .script-result-panel {
    margin: var(--space-2) 0;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-m);
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
  }

  .script-result-title {
    margin: 0 0 var(--space-1-5);
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }

  .script-result-slot {
    font-size: var(--text-s);
    margin: var(--space-1-5) 0;
  }

  .script-result-slot:first-of-type {
    margin-top: 0;
  }

  .script-result-fields {
    list-style: none;
    padding: 0;
    margin: var(--space-1) 0 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    word-break: break-word;
  }

  .script-result-value-line {
    margin: var(--space-1) 0 0;
    word-break: break-word;
  }

  .script-result-value {
    background: var(--color-sunken);
    padding: var(--space-0-5) var(--space-1);
    border-radius: var(--radius-s);
  }

  .condition-group-result h4 {
    font-size: var(--text-m);
    margin: var(--space-3) 0 var(--space-1);
  }

  .condition-eval-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .condition-eval {
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-m);
    border: var(--line-thin) solid var(--color-border);
  }

  .eval-ok {
    border-left: var(--line-stem) solid var(--color-success);
  }

  .eval-fail {
    border-left: var(--line-stem) solid var(--color-danger);
  }

  .eval-line {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }

  .eval-text {
    font-size: var(--text-m);
    word-break: break-word;
  }

  .eval-hint {
    margin: var(--space-1-5) 0 0 var(--space-6);
    font-size: var(--text-s);
    font-style: italic;
    color: var(--color-text-muted);
  }
</style>
