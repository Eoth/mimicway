<script>
  // A translated sentence whose {0}, {1}… placeholders are shown as code: the sentence stays whole for translators,
  // and no markup ever goes through a translation (where it could be broken or injected).
  let { text, codes = [] } = $props();

  let parts = $derived(
    text.split(/(\{\d+\})/).map((part) => {
      const placeholder = part.match(/^\{(\d+)\}$/);
      return placeholder ? { code: codes[Number(placeholder[1])] ?? part } : { text: part };
    }),
  );
</script>

{#each parts as part, i (i)}{#if part.code !== undefined}<code>{part.code}</code>{:else}{part.text}{/if}{/each}
