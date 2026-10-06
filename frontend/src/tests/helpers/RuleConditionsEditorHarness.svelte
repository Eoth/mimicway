<script>
  // Reproduit exactement le cablage reel de RuleForm.svelte
  // (`allOf`/`anyOf` en $state, onAllOfChange/onAnyOfChange qui reassignent)
  // pour tester des scenarios ou l'ordre des mises a jour synchrones importe
  // (ex. suppression d'une condition pendant l'edition d'une autre).
  import { untrack } from 'svelte';
  import RuleConditionsEditor from '../../lib/components/RuleConditionsEditor.svelte';

  let { initialAllOf = [], initialAnyOf = [] } = $props();
  let allOf = $state(untrack(() => initialAllOf));
  let anyOf = $state(untrack(() => initialAnyOf));
</script>

<RuleConditionsEditor {allOf} {anyOf} onAllOfChange={(v) => (allOf = v)} onAnyOfChange={(v) => (anyOf = v)} />
