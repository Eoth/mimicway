<script>
  let { message = '', type = 'info', visible = false } = $props();
</script>

{#if visible}
  <div class="notification {type}" role="alert" aria-live="assertive" aria-atomic="true" data-testid="notification">
    <span class="notification-icon">
      {#if type === 'success'}&#10003;{:else if type === 'error'}&#10007;{:else}&#9432;{/if}
    </span>
    <span class="notification-message">{message}</span>
  </div>
{/if}

<style>
  .notification {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-m);
    font-weight: var(--weight-medium);
    margin-bottom: var(--space-4);
    border: var(--line-thin) solid;
  }

  .notification.success {
    background: var(--color-success-bg);
    border-color: var(--color-success);
    color: var(--color-success-text);
  }

  .notification.error {
    background: var(--color-danger-bg);
    border-color: var(--color-danger);
    color: var(--color-danger-text);
  }

  .notification.info {
    background: var(--color-info-bg);
    border-color: var(--color-info);
    color: var(--color-info-text);
  }

  .notification-icon {
    font-size: var(--text-xl);
    flex-shrink: 0;
  }
</style>
