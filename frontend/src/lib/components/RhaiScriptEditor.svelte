<script>
  // Rhai script editor with a light completion of Mimicway's native functions. The list of functions has one source,
  // ../rhai-functions.js, which also feeds the help under the main script (RuleResponseSection.svelte).
  //
  // A plain <textarea> with a list placed under it, without following the caret's pixel position: the UI has no code
  // editor (CodeMirror, Monaco), and adding one for this alone would be out of proportion.
  //
  // Accessibility: DOM focus stays on the <textarea>, whose implicit "textbox" role supports aria-autocomplete and
  // aria-activedescendant (no explicit role="combobox" needed). The highlighted suggestion is announced through
  // aria-activedescendant, which points to its option in the listbox, never by moving focus.
  import { tick, untrack } from 'svelte';
  import { filterRhaiFunctions, tokenAtCursor, computeInsertSelection } from '../rhai-functions.js';
  import { t } from '../i18n.svelte.js';

  let { id, value = '', onInput = () => {}, rows = 5, placeholder = '', ariaDescribedby = undefined } = $props();

  let textareaEl = $state(null);
  let showSuggestions = $state(false);
  let suggestions = $state([]);
  let activeIndex = $state(0);

  // Read once (untrack): each script slot passes a constant `id`, so an instance never sees it change.
  const listboxId = untrack(() => `${id}-rhai-suggestions`);

  function openSuggestionsFor(text, cursorPos, { allowEmpty = false } = {}) {
    const { token } = tokenAtCursor(text, cursorPos);
    if (!token && !allowEmpty) {
      showSuggestions = false;
      return;
    }
    const matches = filterRhaiFunctions(token);
    if (matches.length === 0) {
      showSuggestions = false;
      return;
    }
    suggestions = matches;
    activeIndex = 0;
    showSuggestions = true;
  }

  function handleInput(e) {
    const newValue = e.target.value;
    onInput(newValue);
    openSuggestionsFor(newValue, e.target.selectionStart);
  }

  function handleKeydown(e) {
    // Ctrl+Space opens the completion even before anything is typed (the whole list), or filtered by the word under
    // the caret.
    if (e.ctrlKey && e.code === 'Space') {
      e.preventDefault();
      openSuggestionsFor(value, e.target.selectionStart, { allowEmpty: true });
      return;
    }
    if (!showSuggestions) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      activeIndex = (activeIndex + 1) % suggestions.length;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      activeIndex = (activeIndex - 1 + suggestions.length) % suggestions.length;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      selectSuggestion(suggestions[activeIndex]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      showSuggestions = false;
    }
  }

  function handleBlur() {
    showSuggestions = false;
  }

  async function selectSuggestion(fn) {
    const el = textareaEl;
    if (!el) return;
    const cursorPos = el.selectionStart;
    const { start } = tokenAtCursor(value, cursorPos);
    const newValue = value.slice(0, start) + fn.insertText + value.slice(cursorPos);
    const sel = computeInsertSelection(fn.insertText);
    showSuggestions = false;
    onInput(newValue);
    // The textarea takes its text from the `value` prop (not bind:value): wait for the next tick before moving the
    // selection, or setSelectionRange would apply to the old content.
    await tick();
    el.focus();
    el.setSelectionRange(start + sel.start, start + sel.end);
  }
</script>

<div class="rhai-editor">
  <textarea
    bind:this={textareaEl}
    {id}
    {value}
    oninput={handleInput}
    onkeydown={handleKeydown}
    onblur={handleBlur}
    {rows}
    class="script-textarea"
    {placeholder}
    aria-describedby={ariaDescribedby}
    aria-autocomplete="list"
    aria-controls={showSuggestions ? listboxId : undefined}
    aria-activedescendant={showSuggestions ? `${listboxId}-opt-${activeIndex}` : undefined}
    data-testid="rhai-script-editor-textarea-{id}"></textarea>
  {#if showSuggestions}
    <ul
      class="rhai-suggestions"
      id={listboxId}
      role="listbox"
      aria-label={t('Available Rhai functions')}
      data-testid="rhai-script-editor-suggestions-{id}"
    >
      {#each suggestions as fn, i (fn.name)}
        <li
          id="{listboxId}-opt-{i}"
          role="option"
          aria-selected={i === activeIndex}
          class="rhai-suggestion"
          class:active={i === activeIndex}
          onmousedown={(e) => {
            e.preventDefault();
            selectSuggestion(fn);
          }}
          onmouseenter={() => (activeIndex = i)}
          data-testid="rhai-script-editor-suggestion-{id}-{fn.name}"
        >
          <code class="rhai-suggestion-sig">{fn.signature}</code>
          <span class="rhai-suggestion-desc">{fn.description}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .rhai-editor {
    position: relative;
  }

  .script-textarea {
    width: 100%;
    font-family: var(--font-code);
    font-size: var(--text-s);
    padding: var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    background: var(--color-bg);
    color: var(--color-text);
    resize: vertical;
    font-variant-ligatures: none;
  }

  .rhai-suggestions {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    z-index: var(--z-popover);
    margin: var(--space-1) 0 0;
    padding: var(--space-1);
    list-style: none;
    max-height: 14rem;
    overflow-y: auto;
    background: var(--color-surface);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-popover);
    max-width: 100%;
    box-sizing: border-box;
  }

  .rhai-suggestion {
    display: flex;
    flex-direction: column;
    gap: var(--space-0-5);
    padding: var(--space-1-5) var(--space-2);
    border-radius: var(--radius-m);
    cursor: pointer;
  }

  .rhai-suggestion.active,
  .rhai-suggestion:hover {
    background: var(--color-selected);
  }

  .rhai-suggestion-sig {
    font-family: var(--font-code);
    font-size: var(--text-s);
    font-weight: var(--weight-strong);
    color: var(--color-primary);
  }

  .rhai-suggestion-desc {
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }

  @media (max-width: 30rem) {
    .rhai-suggestions {
      max-height: 10rem;
    }
  }
</style>
