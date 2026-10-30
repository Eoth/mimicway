<script>
  // A text field of the response builders with help on templates. Two kinds:
  // - `text`, a fixed value: free text in which `{{` opens a value of the request and `|` a pipe; the list suggests
  //   the values, then the pipes, and the line under the field names any expression the engine would render empty
  //   or not as meant;
  // - `pipe`, the pipe chain of a value: the list suggests a pipe after each `|`, and the line under the field shows
  //   what a single pipe does to its example, or what is wrong with the chain.
  // Suggestions, examples and warnings come from ../template-help.js, judged by the engine (catalog_judge.rs).
  //
  // Accessibility as in RhaiScriptEditor.svelte: focus stays on the input, the highlighted suggestion is announced
  // through aria-activedescendant; the notes are tied to the input by aria-describedby.
  //
  // What shows only while the field has the focus (the tip, the list) floats over the page and lets clicks through:
  // in the flow, it would vanish on blur and move the button the pointer is pressing. The warnings stay in the flow,
  // as they change only with the text.
  import { tick } from 'svelte';
  import { t } from '../i18n.svelte.js';
  import {
    suggestAt,
    applySuggestion,
    expressionProblems,
    describePipeProblems,
    pipeExample,
  } from '../template-help.js';
  import { canPipeFixedValue } from '../tpl-utils.js';

  let {
    kind = 'text',
    value = '',
    onInput = () => {},
    placeholder = '',
    ariaLabel,
    testid,
    // `pipe` kind on a fixed value: the value the chain would transform, which the quotes must be able to hold.
    fixedValue = undefined,
  } = $props();

  const uid = $props.id();
  const listboxId = `${uid}-suggestions`;
  const notesId = `${uid}-notes`;
  const tipId = `${uid}-tip`;

  let inputEl = $state(null);
  let focused = $state(false);
  let suggestion = $state(null);
  let activeIndex = $state(0);
  let open = $derived(focused && suggestion !== null && suggestion.items.length > 0);

  // A pipe chain is read as the end of an expression, so that both kinds share one reading of `{{…|…}}`.
  const PIPE_PREFIX = '{{x|';

  function suggestFor(text, caret) {
    if (kind === 'text') return suggestAt(text, caret);
    const s = suggestAt(PIPE_PREFIX + text, caret + PIPE_PREFIX.length);
    return s && { ...s, from: s.from - PIPE_PREFIX.length, caret, close: false };
  }

  let warnings = $derived.by(() => {
    if (kind === 'text') return value?.includes('{{') ? expressionProblems(value) : [];
    if (!value?.trim()) return [];
    const problems = describePipeProblems(value);
    if (fixedValue !== undefined && !canPipeFixedValue(fixedValue)) {
      problems.unshift(
        t('A transformation applies to a plain text: this value holds " \\ { } | ( ) & or <, so it is sent unchanged.'),
      );
    }
    return problems;
  });

  let note = $derived.by(() => {
    if (!focused || warnings.length) return '';
    if (kind === 'text') return t('Type {{ to insert a value of the request, then | to transform it.');
    const example = pipeExample(value);
    return example ? t('For example: {0}', example) : '';
  });

  function refresh(e) {
    suggestion = suggestFor(e.target.value, e.target.selectionStart ?? e.target.value.length);
    activeIndex = 0;
  }

  function handleInput(e) {
    onInput(e.target.value);
    refresh(e);
  }

  function handleKeydown(e) {
    if (!open) return;
    const count = suggestion.items.length;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      activeIndex = (activeIndex + 1) % count;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      activeIndex = (activeIndex - 1 + count) % count;
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      pick(suggestion.items[activeIndex]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      suggestion = null;
    }
  }

  async function pick(item) {
    const el = inputEl;
    const current = suggestion;
    if (!el || !current) return;
    const next = applySuggestion(value ?? '', current, item);
    suggestion = null;
    onInput(next.text);
    // The input takes its text from the `value` prop: wait for it before placing the caret.
    await tick();
    el.focus();
    el.setSelectionRange(next.caret, next.caret);
    suggestion = suggestFor(next.text, next.caret);
    activeIndex = 0;
  }
</script>

<div class="template-text" class:pipe={kind === 'pipe'}>
  <input
    bind:this={inputEl}
    type="text"
    class="template-text-input"
    {value}
    {placeholder}
    aria-label={ariaLabel}
    aria-autocomplete="list"
    aria-controls={open ? listboxId : undefined}
    aria-activedescendant={open ? `${listboxId}-${activeIndex}` : undefined}
    aria-describedby={[warnings.length ? notesId : '', !open && note ? tipId : ''].filter(Boolean).join(' ') ||
      undefined}
    autocomplete="off"
    spellcheck="false"
    oninput={handleInput}
    onkeydown={handleKeydown}
    onclick={refresh}
    onfocus={(e) => {
      focused = true;
      refresh(e);
    }}
    onblur={() => {
      focused = false;
      suggestion = null;
    }}
    data-testid={testid}
  />
  {#if open}
    <ul
      class="template-suggestions"
      id={listboxId}
      role="listbox"
      aria-label={kind === 'pipe' ? t('Transformations') : t('Values of the request and transformations')}
      data-testid="{testid}-suggestions"
    >
      {#each suggestion.items as item, i (item.insert)}
        <li
          id="{listboxId}-{i}"
          role="option"
          aria-selected={i === activeIndex}
          class="template-suggestion"
          class:active={i === activeIndex}
          onmousedown={(e) => {
            e.preventDefault();
            pick(item);
          }}
          onmouseenter={() => (activeIndex = i)}
          data-testid="{testid}-suggestion-{item.insert}"
        >
          <code translate="no">{item.insert}</code>
          <span class="template-suggestion-label">{item.label}</span>
        </li>
      {/each}
    </ul>
  {/if}
  {#if !open && note}
    <p class="template-tip" id={tipId} data-testid="{testid}-tip">{note}</p>
  {/if}
  {#if warnings.length}
    <div class="template-notes" id={notesId} data-testid="{testid}-notes">
      {#each warnings as warning}
        <p class="template-warning">⚠ {warning}</p>
      {/each}
    </div>
  {/if}
</div>

<style>
  .template-text {
    position: relative;
    flex: 1;
    min-width: 6rem;
  }
  .template-text.pipe {
    flex: 0 1 14rem;
    min-width: 8rem;
  }
  .template-text-input {
    width: 100%;
    box-sizing: border-box;
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
  }
  .pipe .template-text-input {
    font-family: var(--font-code);
    color: var(--color-primary);
  }
  .template-suggestions {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: var(--z-popover);
    min-width: 100%;
    width: max-content;
    max-width: min(28rem, 90vw);
    margin: var(--space-1) 0 0;
    padding: var(--space-1);
    list-style: none;
    max-height: 14rem;
    overflow-y: auto;
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-popover);
    box-sizing: border-box;
  }
  .template-suggestion {
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-m);
    cursor: pointer;
  }
  .template-suggestion.active,
  .template-suggestion:hover {
    background: var(--color-selected);
  }
  .template-suggestion code {
    font-family: var(--font-code);
    font-size: var(--text-s);
    color: var(--color-primary);
  }
  .template-suggestion-label {
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }
  .template-notes p {
    margin: var(--space-0-5) 0 0;
    font-size: var(--text-s);
  }
  .template-tip {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: var(--z-popover);
    width: max-content;
    max-width: min(28rem, 90vw);
    margin: var(--space-1) 0 0;
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-s);
    color: var(--color-text-muted);
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-popover);
    pointer-events: none;
  }
  .template-warning {
    color: var(--color-warning-text);
  }
</style>
