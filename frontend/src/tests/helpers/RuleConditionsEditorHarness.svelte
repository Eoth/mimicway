<script>
  // Wires `RuleConditionsEditor` as `RuleForm.svelte` does (`allOf` and `anyOf` in `$state`, reassigned by the change
  // callbacks), for tests where the order of synchronous updates matters, such as removing a condition while another
  // one is being edited.
  import { untrack } from 'svelte';
  import RuleConditionsEditor from '../../lib/components/RuleConditionsEditor.svelte';

  let { initialAllOf = [], initialAnyOf = [] } = $props();
  let allOf = $state(untrack(() => initialAllOf));
  let anyOf = $state(untrack(() => initialAnyOf));
</script>

<RuleConditionsEditor {allOf} {anyOf} onAllOfChange={(v) => (allOf = v)} onAnyOfChange={(v) => (anyOf = v)} />
