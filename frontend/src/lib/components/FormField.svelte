<script>
  // Label, hint and error around any kind of control: the caller renders the control (input, select, textarea) in the
  // `children` snippet, which receives { id, describedBy, invalid } to set on it (for and id, aria-describedby,
  // aria-invalid), so that no form repeats that wiring.
  let { id, label, hint = '', error = '', required = false, checkbox = false, children } = $props();

  let hintId = $derived(hint ? `${id}-hint` : undefined);
  let errorId = $derived(error ? `${id}-error` : undefined);
  let describedBy = $derived([hintId, errorId].filter(Boolean).join(' ') || undefined);
</script>

<div class="form-field" class:form-field-check={checkbox}>
  <label for={id}
    >{label}{#if required}<span aria-hidden="true"> *</span>{/if}</label
  >
  {@render children?.({ id, describedBy, invalid: !!error })}
  {#if hint}
    <span class="field-hint" id={hintId} data-testid="form-field-hint-{id}">{hint}</span>
  {/if}
  {#if error}
    <span class="form-error" id={errorId} role="alert" data-testid="form-field-error-{id}">{error}</span>
  {/if}
</div>
