<script>
  import { t } from '../i18n.svelte.js';
  // The action of a rule, mock or proxy. Controlled: the value comes in through `action`, changes go out through
  // `onChange`. On a purely mocked service the proxy option is removed rather than disabled: there is no target to
  // forward to.
  let { action = 'mock', isPurelyMocked = false, onChange = () => {} } = $props();
</script>

<fieldset class="section action-section">
  <legend>{t('Action when this rule matches')}</legend>
  {#if isPurelyMocked}
    <p class="section-help" data-testid="rule-form-purely-mocked-hint">
      {t('This service is purely mocked (no target configured): only the Mock action is available.')}
    </p>
  {/if}
  <div class="action-selector">
    <label class="action-option mock" class:selected={action === 'mock'} data-testid="rule-form-action-mock-option">
      <input
        type="radio"
        name="rule-action"
        checked={action === 'mock'}
        onchange={() => onChange('mock')}
        data-testid="rule-form-action-mock-radio"
      />
      <span class="action-label">{t('Mock')}</span>
      <span class="action-desc">{t('Return the simulated response below')}</span>
    </label>
    {#if !isPurelyMocked}
      <label
        class="action-option proxy"
        class:selected={action === 'proxy'}
        data-testid="rule-form-action-proxy-option"
      >
        <input
          type="radio"
          name="rule-action"
          checked={action === 'proxy'}
          onchange={() => onChange('proxy')}
          data-testid="rule-form-action-proxy-radio"
        />
        <span class="action-label">{t('Proxy')}</span>
        <span class="action-desc">{t('Forward to the real target of the service')}</span>
      </label>
    {/if}
  </div>
</fieldset>

<style>
  .action-selector {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .action-option {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-0-5);
    padding: var(--space-3) var(--space-4);
    border: var(--line-thick) solid var(--color-control);
    border-radius: var(--radius-m);
    cursor: pointer;
    min-width: 10rem;
    background: var(--color-surface);
  }
  /* The chosen action is drawn in its mode: dashed for the mock that imitates, solid for the real target. */
  .action-option.mock.selected {
    border-style: dashed;
    border-color: var(--color-mock);
    background: var(--color-mock-bg);
  }
  .action-option.proxy.selected {
    border-color: var(--color-proxy);
    background: var(--color-proxy-bg);
  }
  /* The card shows the choice, so the radio button itself is hidden, but only visually: `display: none` would take it
     out of the keyboard's reach. The card shows its focus instead. */
  .action-option input {
    position: absolute;
    opacity: 0;
    width: 1px;
    height: 1px;
    margin: 0;
    pointer-events: none;
  }
  .action-option:has(input:focus-visible) {
    outline: var(--line-thick) solid var(--color-focus);
    outline-offset: var(--line-thick);
  }
  .action-label {
    font-weight: var(--weight-heavy);
    font-size: var(--text-m);
  }
  .action-desc {
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }
</style>
