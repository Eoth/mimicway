<script>
  // One Rhai script slot: toggle, editor, validation and help. RuleResponseSection.svelte uses it three times
  // (pre_script, script, post_script: same layout, run independently, no chaining). The caller passes the help through
  // the `help` snippet, since it differs per slot: a short text shared by pre_script and post_script, examples and the
  // list of functions for the main script.
  import ToggleSwitch from './ToggleSwitch.svelte';
  import RhaiScriptEditor from './RhaiScriptEditor.svelte';
  import { t } from '../i18n.svelte.js';

  let {
    id,
    toggleLabel,
    code = '',
    enabled = false,
    onToggle = () => {},
    onCodeInput = () => {},
    validation = { status: '', message: '' },
    onValidate = () => {},
    rows = 5,
    placeholder = '',
    help,
  } = $props();
</script>

<div class="sub-section script-section">
  <ToggleSwitch label={toggleLabel} name={id} checked={enabled} onchange={onToggle} />
  {#if enabled}
    <div class="script-editor">
      <label for={id}>{t('Rhai code')}</label>
      <RhaiScriptEditor {id} value={code} onInput={onCodeInput} {rows} {placeholder} ariaDescribedby="{id}-hint" />
      <div class="script-actions">
        <button
          type="button"
          class="btn btn-outline btn-sm"
          onclick={onValidate}
          disabled={validation.status === 'pending'}
          data-testid="rule-form-validate-script-button-{id}"
        >
          {validation.status === 'pending' ? t('Validating...') : t('Validate the script')}
        </button>
        {#if validation.status === 'ok'}
          <span class="script-valid" role="status" data-testid="rule-form-script-valid-{id}"
            >&#10003; {validation.message}</span
          >
        {:else if validation.status === 'error'}
          <span class="script-invalid" role="alert" data-testid="rule-form-script-invalid-{id}"
            >{validation.message}</span
          >
        {/if}
      </div>
      <div class="script-help" id="{id}-hint">
        {@render help()}
      </div>
    </div>
  {/if}
</div>

<style>
  .script-section {
    border-top-color: var(--color-primary);
  }
  .script-editor {
    margin-top: var(--space-3);
  }
  .script-editor label {
    display: block;
    font-weight: var(--weight-strong);
    font-size: var(--text-m);
    margin-bottom: var(--space-1);
  }
  .script-actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-1-5);
  }
  .script-valid {
    font-size: var(--text-s);
    color: var(--color-success);
    font-weight: var(--weight-strong);
  }
  .script-invalid {
    font-size: var(--text-s);
    color: var(--color-danger);
  }
  .script-help {
    margin-top: var(--space-1-5);
  }

  /* The `help` snippet is defined in RuleResponseSection.svelte, so its elements carry that component's scope class,
     not this one's: :global() lets these rules reach them across the component boundary. */
  .script-help :global(p) {
    margin: var(--space-1) 0;
  }
  .script-help :global(code) {
    font-size: var(--text-s);
    background: var(--color-bg);
    padding: var(--space-0-5) var(--space-1);
    border-radius: var(--radius-s);
  }
  .script-help :global(.script-examples) {
    margin-top: var(--space-1-5);
  }
  .script-help :global(.script-examples summary) {
    cursor: pointer;
    color: var(--color-primary);
    font-size: var(--text-s);
  }
  .script-help :global(.script-examples-content) {
    padding: var(--space-2);
    background: var(--color-bg);
    border-radius: var(--radius-m);
    margin-top: var(--space-1);
    font-size: var(--text-s);
  }
  .script-help :global(.script-examples-content p) {
    margin: var(--space-1) 0;
  }
  .script-help :global(.script-examples-content a) {
    color: var(--color-primary);
  }
  .script-help :global(.script-fn-list) {
    margin: var(--space-1) 0 var(--space-2);
    padding-left: var(--space-4);
  }
  .script-help :global(.script-fn-list li) {
    margin: var(--space-0-5) 0;
  }
</style>
