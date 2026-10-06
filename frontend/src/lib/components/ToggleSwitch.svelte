<script>
  import { t } from '../i18n.svelte.js';
  // `name` identifies the switch (DOM ids, test id) whatever the language of its label.
  let { checked = false, label = '', name = null, disabled = false, onchange = () => {} } = $props();
  let key = $derived(name ?? label.replace(/\s+/g, '-'));

  function handleClick() {
    if (disabled) return;
    onchange(!checked);
  }

  function handleKeydown(e) {
    if (e.key === ' ' || e.key === 'Enter') {
      e.preventDefault();
      handleClick();
    }
  }
</script>

<div class="toggle-wrapper">
  <span class="toggle-label" id="toggle-label-{key}">{label}</span>
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-labelledby="toggle-label-{key}"
    class="toggle-switch"
    class:active={checked}
    {disabled}
    onclick={handleClick}
    onkeydown={handleKeydown}
    data-testid="toggle-switch-{key}"
  >
    <span class="toggle-knob"></span>
    <span class="sr-only">{checked ? t('Enabled') : t('Disabled')}</span>
  </button>
  <span class="toggle-status" aria-live="polite">
    {checked ? t('ON') : t('OFF')}
  </span>
</div>

<style>
  .toggle-wrapper {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .toggle-label {
    font-weight: var(--weight-medium);
    color: var(--color-text);
  }

  .toggle-switch {
    position: relative;
    width: 52px;
    height: 28px;
    border-radius: var(--radius-pill);
    border: var(--line-thick) solid var(--color-control);
    background: var(--color-sunken);
    padding: 0;
    transition:
      background-color var(--duration-move),
      border-color var(--duration-move);
  }

  .toggle-switch.active {
    background: var(--color-mock);
    border-color: var(--color-mock);
  }

  .toggle-switch:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .toggle-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 20px;
    height: 20px;
    border-radius: var(--radius-round);
    /* Off, the knob takes the control color so that it stands out from the track in both themes (3:1). */
    background: var(--color-control);
    transition: transform var(--duration-move);
  }

  .toggle-switch.active .toggle-knob {
    background: var(--color-surface);
    transform: translateX(24px);
  }

  .toggle-status {
    font-size: var(--text-m);
    font-weight: var(--weight-strong);
    min-width: 2rem;
    color: var(--color-text-muted);
  }

  .toggle-switch.active + .toggle-status {
    color: var(--color-mock);
  }
</style>
