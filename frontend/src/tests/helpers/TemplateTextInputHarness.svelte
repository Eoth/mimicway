<script>
  // Holds the value as a parent would, so that the input shows what the component reported through onInput.
  import TemplateTextInput from '../../lib/components/TemplateTextInput.svelte';
  let { kind = 'text', initial = '', fixedValue = undefined } = $props();
  let value = $state(initial);
</script>

<TemplateTextInput {kind} {value} {fixedValue} onInput={(v) => (value = v)} ariaLabel="Value" testid="tti" />
<output data-testid="tti-value">{value}</output>
