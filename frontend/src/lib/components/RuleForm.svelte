<script>
  // The rule form: owns the rule's identity (name, method, sub-path), its action and its conditions, and runs the save
  // flow (local checks, the warning for a stale proxy rule, the conflict check). Each section has its own component:
  //   - RuleActionSelector.svelte: mock or proxy;
  //   - RuleConditionsEditor.svelte: the AND and OR condition fieldsets;
  //   - RuleResponseSection.svelte: the "Mocked response" fieldset (format, headers, body, the three script slots,
  //     chaos), read through getPayload() and validate();
  //   - RuleWarnings.svelte: the non-blocking warnings (stale proxy rule, conflicting rules);
  //   - RuleTester.svelte: replays the draft rule against a captured request.
  import RuleActionSelector from './RuleActionSelector.svelte';
  import RuleConditionsEditor from './RuleConditionsEditor.svelte';
  import RuleResponseSection from './RuleResponseSection.svelte';
  import RuleWarnings from './RuleWarnings.svelte';
  import RuleTester from './RuleTester.svelte';
  import { getLogs, checkRuleConflicts } from '../api.js';
  import { combinePathParamNames } from '../path-params.js';
  import { t } from '../i18n.svelte.js';

  import { untrack } from 'svelte';

  let {
    rule = null,
    existingRules = [],
    // Where the rule will sit once saved: its index when edited, or service.rules.length when added (handleSaveRule in
    // ServiceDetail.svelte appends it). The conflict check needs it to tell which of two overlapping rules applies.
    // Without it (a form rendered alone, in a unit test for instance), the rule is taken as added at the end.
    draftPosition = null,
    serviceName = null,
    groupName = null,
    listenPath = '',
    // A purely mocked service has no target to forward to: the proxy action is removed rather than disabled.
    isPurelyMocked = false,
    onSave = () => {},
    onCancel = () => {},
  } = $props();

  // existingRuleNames (for the uniqueness of the name) derives from existingRules (also read by the conflict check)
  // rather than coming as a prop of its own, so the two lists cannot diverge.
  let existingRuleNames = $derived(existingRules.map((r) => r.name));
  let effectiveDraftPosition = $derived(draftPosition ?? existingRules.length);

  const init = untrack(() => (rule ? JSON.parse(JSON.stringify(rule)) : null));
  let name = $state(init?.name ?? '');
  let ruleMethod = $state(init?.method ?? 'GET');
  let subPath = $state(init?.sub_path ?? '');
  // A proxy rule opened on a purely mocked service can only be a mock: RuleActionSelector removes the proxy option, and
  // the form must not hold a value that no visible radio button shows.
  let ruleAction = $state(
    untrack(() => (isPurelyMocked && init?.action === 'proxy' ? 'mock' : (init?.action ?? 'mock'))),
  );
  // True only for such a rule (stored as proxy, shown as mock). Fixed for the life of the form: it never depends on
  // `ruleAction`, which cannot go back to proxy while that option is removed. It only triggers the save warning below
  // and never changes `ruleAction`.
  const isStaleProxyRule = untrack(() => isPurelyMocked && init?.action === 'proxy');

  const httpMethods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS', 'HEAD'];
  let allOf = $state(init?.conditions?.all_of ?? []);
  let anyOf = $state(init?.conditions?.any_of ?? []);

  // Input help for conditions: the path parameters of the service's path and of the rule's sub-path (a closed list),
  // and the query parameters seen in the service's traffic, from one load of the logs (no request per field).
  let availablePathParams = $derived(combinePathParamNames([listenPath, subPath]));

  let serviceLogs = $state([]);
  async function loadServiceLogs() {
    if (!serviceName) return;
    try {
      const logs = await getLogs(200);
      // A name alone does not identify a service: another group may hold one of the same name.
      serviceLogs = logs.filter(
        (l) => l.service_name === serviceName && (l.group_name ?? null) === (groupName ?? null),
      );
    } catch {
      serviceLogs = [];
    }
  }
  $effect(() => {
    loadServiceLogs();
  });

  let queryParamSuggestions = $derived(
    [...new Set(serviceLogs.flatMap((l) => Object.keys(l.captured?.query_params ?? {})))].sort(),
  );

  let formError = $state('');

  // The "Mocked response" fieldset stays mounted whatever `ruleAction` is, so that switching to proxy and back keeps
  // what it holds (see RuleResponseSection.svelte): getPayload() can always be called; validate() is called for a mock
  // rule only.
  let responseSectionRef = $state(null);

  // Conflict check, on save only: when the draft overlaps another rule of the service, a non-blocking warning offers
  // "Save anyway" (saves at once) or "Edit the rule" (closes the warning, the form stays open).
  let pendingConflicts = $state([]);
  let pendingRulePayload = $state(null);
  let checkingConflicts = $state(false);

  // Warning that saving really changes the action: for `isStaleProxyRule` only, never for an ordinary mock rule nor for
  // a proxy rule of a service that has a target. Non-blocking, like the conflict warning above and the purely mocked
  // warning of ServiceForm.svelte: "Save anyway" goes on, "Edit the rule" closes it without saving.
  let pendingStaleProxyWarning = $state(false);
  let pendingStaleProxyPayload = $state(null);

  function buildRulePayload() {
    // Built whatever `ruleAction` is: a proxy rule keeps its response and scripts, ready if it becomes a mock again.
    const { response, pre_script, script, post_script, response_mode } = responseSectionRef.getPayload();
    return {
      name: name.trim(),
      method: ruleMethod,
      sub_path: subPath.trim() || null,
      action: ruleAction,
      pre_script,
      script,
      post_script,
      response_mode,
      conditions: { all_of: allOf, any_of: anyOf },
      response,
    };
  }

  async function handleSubmit(e) {
    e.preventDefault();
    formError = '';
    pendingConflicts = [];
    pendingRulePayload = null;
    pendingStaleProxyWarning = false;
    pendingStaleProxyPayload = null;

    const trimmedName = name.trim();
    if (!trimmedName) {
      formError = t('The rule name is required.');
      return;
    }
    if (existingRuleNames.some((n) => n.toLowerCase() === trimmedName.toLowerCase())) {
      formError = t('A rule named "{0}" already exists in this service.', trimmedName);
      return;
    }

    if (ruleAction === 'mock') {
      const validationErr = responseSectionRef.validate();
      if (validationErr) {
        formError = validationErr;
        return;
      }
    }

    const builtRule = buildRulePayload();

    // A local check, before the conflict check: the server is not asked anything until the user confirms the change of
    // action (proxy to mock).
    if (isStaleProxyRule) {
      pendingStaleProxyWarning = true;
      pendingStaleProxyPayload = builtRule;
      return;
    }

    await checkConflictsAndSave(builtRule);
  }

  async function checkConflictsAndSave(builtRule) {
    checkingConflicts = true;
    try {
      const result = await checkRuleConflicts({
        draft: {
          method: builtRule.method,
          sub_path: builtRule.sub_path,
          conditions: builtRule.conditions,
        },
        other_rules: existingRules.map((r) => ({
          name: r.name,
          method: r.method,
          sub_path: r.sub_path,
          conditions: r.conditions,
        })),
        draft_position: effectiveDraftPosition,
      });
      const conflicts = result.conflicts ?? [];
      if (conflicts.length === 0) {
        onSave(builtRule);
      } else {
        pendingConflicts = conflicts;
        pendingRulePayload = builtRule;
      }
    } catch {
      // Fail open: the conflict check only informs, so its failure must never prevent saving the rule.
      onSave(builtRule);
    } finally {
      checkingConflicts = false;
    }
  }

  function confirmSaveDespiteStaleProxyWarning() {
    const payload = pendingStaleProxyPayload;
    pendingStaleProxyWarning = false;
    pendingStaleProxyPayload = null;
    if (payload) checkConflictsAndSave(payload);
  }

  function cancelStaleProxyWarning() {
    pendingStaleProxyWarning = false;
    pendingStaleProxyPayload = null;
  }

  function confirmSaveDespiteConflicts() {
    if (pendingRulePayload) onSave(pendingRulePayload);
    pendingConflicts = [];
    pendingRulePayload = null;
  }

  function dismissConflictWarning() {
    pendingConflicts = [];
    pendingRulePayload = null;
  }
</script>

<form class="rule-form" onsubmit={handleSubmit} aria-label={init ? t('Edit the rule {0}', init.name) : t('New rule')}>
  {#if formError}
    <div class="form-error" role="alert" aria-live="assertive">{formError}</div>
  {/if}

  <div class="form-field">
    <label for="rule-name">{t('Rule name')}</label>
    <input
      id="rule-name"
      type="text"
      bind:value={name}
      required
      placeholder={t('e.g. get-customer')}
      aria-describedby="rn-hint"
      data-testid="rule-form-name-input"
    />
    <span class="field-hint" id="rn-hint">{t('Unique identifier of this rule in the service')}</span>
  </div>

  <div class="form-row">
    <div class="form-field">
      <label for="rule-method">{t('HTTP method')}</label>
      <select
        id="rule-method"
        bind:value={ruleMethod}
        aria-describedby="rule-method-hint"
        data-testid="rule-form-method-select"
      >
        {#each httpMethods as m}
          <option value={m}>{m}</option>
        {/each}
      </select>
      <span class="field-hint" id="rule-method-hint">{t('HTTP method this rule intercepts')}</span>
    </div>

    <div class="form-field">
      <label for="rule-subpath">{t('Sub-path (optional)')}</label>
      <input
        id="rule-subpath"
        type="text"
        bind:value={subPath}
        placeholder={t('e.g. /users/{id}')}
        aria-describedby="rule-subpath-hint"
        data-testid="rule-form-subpath-input"
      />
      <span class="field-hint" id="rule-subpath-hint">{t('Narrows the matching within the service')}</span>
    </div>
  </div>

  <RuleActionSelector action={ruleAction} {isPurelyMocked} onChange={(v) => (ruleAction = v)} />

  {#if serviceName}
    <RuleTester
      {serviceName}
      {groupName}
      logs={serviceLogs}
      getDraftRule={() => {
        // The action and the three script slots too, not only method, sub-path and conditions: the tester runs the
        // scripts against the chosen captured request. responseSectionRef is null until bind:this has run, which a
        // click on "Test" cannot precede in practice; the guard costs nothing.
        const payload = responseSectionRef?.getPayload() ?? {};
        return {
          method: ruleMethod,
          subPath,
          allOf,
          anyOf,
          action: ruleAction,
          preScript: payload.pre_script ?? null,
          script: payload.script ?? null,
          postScript: payload.post_script ?? null,
        };
      }}
    />
  {/if}

  <RuleConditionsEditor
    {allOf}
    {anyOf}
    {availablePathParams}
    {queryParamSuggestions}
    onAllOfChange={(v) => (allOf = v)}
    onAnyOfChange={(v) => (anyOf = v)}
  />

  <RuleResponseSection bind:this={responseSectionRef} visible={ruleAction === 'mock'} initRule={init} />

  <RuleWarnings
    {pendingStaleProxyWarning}
    onConfirmStaleProxy={confirmSaveDespiteStaleProxyWarning}
    onCancelStaleProxy={cancelStaleProxyWarning}
    {pendingConflicts}
    onConfirmConflicts={confirmSaveDespiteConflicts}
    onDismissConflicts={dismissConflictWarning}
  />

  <div class="form-actions">
    <button type="submit" class="btn btn-primary" disabled={checkingConflicts} data-testid="rule-form-submit-button">
      {#if checkingConflicts}{t('Checking…')}{:else}{init ? t('Save the rule') : t('Add the rule')}{/if}
    </button>
    <button type="button" class="btn btn-secondary" onclick={onCancel} data-testid="rule-form-cancel-button"
      >{t('Cancel')}</button
    >
  </div>
</form>

<style>
  .rule-form {
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-5);
  }
</style>
