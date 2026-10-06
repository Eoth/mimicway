<script>
  import { t, tCount } from '../i18n.svelte.js';
  // Shows the two non-blocking warnings of saving a rule:
  // - stale proxy: a rule saved with the proxy action, reopened after its service became purely mocked (saving it
  //   really turns it into a mock);
  // - conflict: the rule overlaps another rule of the service (POST /api/rule-conflicts).
  // No state of its own: RuleForm.svelte decides when each warning shows (the local check, then the server one, in
  // handleSubmit).
  let {
    pendingStaleProxyWarning = false,
    onConfirmStaleProxy = () => {},
    onCancelStaleProxy = () => {},
    pendingConflicts = [],
    onConfirmConflicts = () => {},
    onDismissConflicts = () => {},
  } = $props();
</script>

{#if pendingStaleProxyWarning}
  <div class="callout callout-warning" role="alert" data-testid="rule-form-stale-proxy-warning">
    <p class="callout-title">
      {t(
        '⚠ This rule was saved with the “Proxy” action, but the service is now purely mocked: saving it now really switches it to “Mock” (it will never forward to a target again).',
      )}
    </p>
    <div class="callout-actions">
      <button
        type="button"
        class="btn btn-sm btn-primary"
        onclick={onConfirmStaleProxy}
        data-testid="rule-form-stale-proxy-save-anyway-button">{t('Save anyway')}</button
      >
      <button
        type="button"
        class="btn btn-sm btn-secondary"
        onclick={onCancelStaleProxy}
        data-testid="rule-form-stale-proxy-cancel-button">{t('Edit the rule')}</button
      >
    </div>
  </div>
{/if}

{#if pendingConflicts.length > 0}
  <div class="callout callout-warning" role="alert" data-testid="rule-form-conflict-warning">
    <p class="callout-title">
      {tCount(
        pendingConflicts.length,
        '⚠ This rule may conflict with an existing rule of this service:',
        '⚠ This rule may conflict with these existing rules of this service:',
      )}
    </p>
    <ul class="callout-list">
      {#each pendingConflicts as c (c.other_rule_name)}
        <li data-testid="rule-form-conflict-warning-item-{c.other_rule_name}">
          {#if c.winner === 'other'}
            {t(
              'The rule “{0}” has identical or included conditions: in the current order, “{0}” applies, and this rule never triggers for those requests.',
              c.other_rule_name,
            )}
          {:else}
            {t(
              'The rule “{0}” has identical or included conditions: in the current order, this rule applies first, and “{0}” is ignored for those requests.',
              c.other_rule_name,
            )}
          {/if}
        </li>
      {/each}
    </ul>
    <p class="field-hint">
      {t(
        'The conflict may be intended (a general rule with a more specific one as fallback, for instance). You may still save.',
      )}
    </p>
    <div class="callout-actions">
      <button
        type="button"
        class="btn btn-sm btn-primary"
        onclick={onConfirmConflicts}
        data-testid="rule-form-conflict-save-anyway-button">{t('Save anyway')}</button
      >
      <button
        type="button"
        class="btn btn-sm btn-secondary"
        onclick={onDismissConflicts}
        data-testid="rule-form-conflict-cancel-button">{t('Edit the rule')}</button
      >
    </div>
  </div>
{/if}
