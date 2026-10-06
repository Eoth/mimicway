<script>
  import { t } from '../i18n.svelte.js';
  let { active = false } = $props();
</script>

<span
  class="badge-pill {active ? 'badge-mock' : 'badge-proxy'}"
  role="status"
  aria-label={active ? t('Mock mode on') : t('Proxy mode on')}
  data-testid="status-badge"
>
  {active ? t('MOCK') : t('PROXY')}
</span>
