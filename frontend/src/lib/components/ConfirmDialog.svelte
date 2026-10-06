<script>
  // The confirmation dialog of destructive actions (deleting a service or a group, resetting or restoring the
  // configuration). It uses the dialog classes of app.css (.modal-overlay, .modal-content, .modal-header,
  // .modal-footer, .btn-close) and has no style of its own.
  import { tick } from 'svelte';
  import { t } from '../i18n.svelte.js';

  let {
    open = false,
    title = null,
    message = '',
    confirmLabel = null,
    cancelLabel = null,
    danger = true,
    confirmKeyword = null,
    onConfirm = () => {},
    onCancel = () => {},
  } = $props();

  let keywordInput = $state('');
  let confirmBtn = $state();
  let keywordInputEl = $state();

  $effect(() => {
    if (open) {
      keywordInput = '';
      tick().then(() => {
        (confirmKeyword ? keywordInputEl : confirmBtn)?.focus();
      });
    }
  });

  let canConfirm = $derived(!confirmKeyword || keywordInput === confirmKeyword);

  function handleKeydown(e) {
    if (e.key === 'Escape') onCancel();
  }
  function handleBackdrop(e) {
    if (e.target === e.currentTarget) onCancel();
  }
  function handleConfirm() {
    if (canConfirm) onConfirm();
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="modal-overlay"
    role="dialog"
    aria-modal="true"
    aria-labelledby="confirm-dialog-title"
    tabindex="-1"
    onkeydown={handleKeydown}
    onclick={handleBackdrop}
    data-testid="confirm-dialog"
  >
    <div class="modal-content" role="document">
      <div class="modal-header">
        <h3 id="confirm-dialog-title">{title ?? t('Confirm')}</h3>
        <button
          type="button"
          class="btn-close"
          onclick={onCancel}
          aria-label={t('Close')}
          data-testid="confirm-dialog-close-button">&#10005;</button
        >
      </div>
      <p>{message}</p>
      {#if confirmKeyword}
        <div class="form-field">
          <label for="confirm-keyword-input">{t('Type “{0}” to confirm', confirmKeyword)}</label>
          <input
            id="confirm-keyword-input"
            type="text"
            bind:value={keywordInput}
            bind:this={keywordInputEl}
            autocomplete="off"
            spellcheck="false"
            data-testid="confirm-dialog-keyword-input"
          />
        </div>
      {/if}
      <div class="modal-footer">
        <button type="button" class="btn btn-secondary" onclick={onCancel} data-testid="confirm-dialog-cancel-button"
          >{cancelLabel ?? t('Cancel')}</button
        >
        <button
          type="button"
          class={danger ? 'btn btn-danger' : 'btn btn-primary'}
          bind:this={confirmBtn}
          onclick={handleConfirm}
          disabled={!canConfirm}
          data-testid="confirm-dialog-confirm-button"
        >
          {confirmLabel ?? t('Confirm')}
        </button>
      </div>
    </div>
  </div>
{/if}
