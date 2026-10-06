<script>
  import { t, tCount } from '../i18n.svelte.js';
  // The by-example JSON view: edits only the values (source, pipe) of the fields found in the pasted sample. No key is
  // renamed, added, removed, moved or retyped here, unlike the detailed view (JsonResponseBuilder.svelte), which edits
  // the same shape of fields in full. The "Edit in detail" button of RuleResponseSection.svelte hands this component's
  // `fields` array to that view as it is.
  import { untrack } from 'svelte';
  import { buildExpr as sharedBuildExpr, fieldsToTemplate, exampleJsonToFields } from '../tpl-utils.js';

  // startParsed: the parent already holds fields (rebuilt from a saved template by computeInitialEditorState in
  // RuleResponseSection.svelte, or kept from the other view), so the list of fields shows instead of the paste area.
  // Read once, when the component is created (untrack): a later change of this prop must not open or close the paste
  // area behind the user's back. `arrayRoot` likewise: the parent keeps whether the sample was an array, since this
  // component is unmounted whenever the response section is folded or the view changes.
  let {
    fields = [],
    startParsed = false,
    arrayRoot = false,
    onUpdate = () => {},
    onArrayRootChange = () => {},
  } = $props();

  let pasteInput = $state('');
  let parseError = $state('');
  let parsed = $state(untrack(() => startParsed));
  let isArrayRoot = $state(untrack(() => arrayRoot));

  // Folding of 'object' fields, the only nested type this view renders (an array field shows an "array" badge only).
  // As in XmlPasteBuilder.svelte and JsonResponseBuilder.svelte: an in-memory Set keyed by the positional test path,
  // everything unfolded at first, and a folded subtree hidden with the `hidden` attribute.
  let collapsedPaths = $state(new Set());

  function isCollapsed(testPath) {
    return collapsedPaths.has(testPath);
  }

  function toggleCollapse(testPath) {
    const next = new Set(collapsedPaths);
    if (next.has(testPath)) next.delete(testPath);
    else next.add(testPath);
    collapsedPaths = next;
  }

  const valueSources = [
    {
      value: 'fixed',
      get label() {
        return t('Keep the value');
      },
    },
    {
      value: 'path',
      get label() {
        return t('URL parameter');
      },
    },
    {
      value: 'query',
      get label() {
        return t('Query param');
      },
    },
    {
      value: 'header',
      get label() {
        return t('HTTP header');
      },
    },
    {
      value: 'body',
      get label() {
        return t('Echo of the body');
      },
    },
    {
      value: 'fake',
      get label() {
        return t('Fake data');
      },
    },
    {
      value: 'uuid',
      get label() {
        return t('UUID');
      },
    },
    {
      value: 'now_ms',
      get label() {
        return t('Timestamp (ms)');
      },
    },
    {
      value: 'now_iso',
      get label() {
        return t('ISO date');
      },
    },
    {
      value: 'seq',
      get label() {
        return t('Counter');
      },
    },
    {
      value: 'script',
      get label() {
        return t('Script result');
      },
    },
  ];

  const fakeOptions = [
    'FirstName',
    'LastName',
    'Email',
    'PhoneNumberFR',
    'CompanyName',
    'StreetName',
    'CityFR',
    'PostcodeFR',
    'Siren',
    'Siret',
    'FullAddressFR',
    'DatePast',
    'DateFuture',
    'TimestampMs',
    'BoolRandom',
    'LoremSentence',
    'CountryFR',
    'IbanFR',
  ];

  // The pipe suggestions of JsonResponseBuilder.svelte: each one is a pipe of the template engine.
  const pipeOptions = [
    {
      value: 'lower',
      get label() {
        return t('lower — lowercase');
      },
    },
    {
      value: 'upper',
      get label() {
        return t('upper — uppercase');
      },
    },
    {
      value: 'trim',
      get label() {
        return t('trim — strip spaces');
      },
    },
    {
      value: 'capitalize',
      get label() {
        return t('capitalize — first letter uppercase');
      },
    },
    {
      value: 'first(N)',
      get label() {
        return t('first(N) — first N characters');
      },
    },
    {
      value: 'last(N)',
      get label() {
        return t('last(N) — last N characters');
      },
    },
    {
      value: 'substr(start,len)',
      get label() {
        return t('substr(start,len)');
      },
    },
    {
      value: 'default("val")',
      get label() {
        return t('default("val") — when empty');
      },
    },
    {
      value: 'replace("a","b")',
      get label() {
        return t('replace("a","b")');
      },
    },
    {
      value: 'prepend("prefix")',
      get label() {
        return t('prepend("prefix")');
      },
    },
    {
      value: 'append("suffix")',
      get label() {
        return t('append("suffix")');
      },
    },
    {
      value: 'length',
      get label() {
        return t('length — number of characters');
      },
    },
  ];

  function handleParse() {
    parseError = '';
    const text = pasteInput.trim();
    if (!text) {
      parseError = t('Paste valid JSON.');
      return;
    }
    try {
      const data = JSON.parse(text);
      if (Array.isArray(data)) {
        if (data.length === 0) {
          parseError = t('The array is empty. Paste an array with at least one element.');
          return;
        }
        isArrayRoot = true;
        fields = exampleJsonToFields(typeof data[0] === 'object' && data[0] !== null ? data[0] : { value: data[0] });
      } else if (typeof data === 'object' && data !== null) {
        isArrayRoot = false;
        fields = exampleJsonToFields(data);
      } else {
        parseError = t('The JSON must be an object or an array.');
        return;
      }
      parsed = true;
      onArrayRootChange(isArrayRoot);
      emit();
    } catch (e) {
      parseError = t('Invalid JSON: {0}', e.message);
    }
  }

  function deepClone(obj) {
    return JSON.parse(JSON.stringify(obj));
  }

  function updateField(path, prop, val) {
    const clone = deepClone(fields);
    let target = clone;
    for (let i = 0; i < path.length - 1; i++) target = target[path[i]];
    const field = target[path[path.length - 1]];
    field[prop] = val;
    if (prop === 'source' && val === 'fake') field.value = 'CompanyName';
    if (prop === 'source' && ['uuid', 'now_ms', 'now_iso', 'seq'].includes(val)) field.value = '';
    fields = clone;
    emit();
  }

  function emit() {
    onUpdate(fields);
  }

  function needsValueInput(src) {
    return ['fixed', 'path', 'query', 'header', 'body', 'script'].includes(src);
  }

  // As in JsonResponseBuilder.svelte and XmlResponseBuilder.svelte: a script result is read one flat key deep
  // ({{script.field}}), never through a nested path.
  function valuePlaceholder(src) {
    if (src === 'fixed') return t('fixed value');
    if (src === 'script')
      return t('e.g. name (empty = the whole {{script}}; one level only: test the rule to see the keys)');
    return t('parameter name');
  }

  export function toTemplate() {
    const obj = fieldsToTemplate(fields);
    return isArrayRoot ? `[${obj}]` : obj;
  }
</script>

<div class="paste-builder" aria-label={t('JSON builder by example')}>
  {#if !parsed}
    <div class="paste-zone">
      <label for="json-paste-input">{t('Paste an example of the JSON response')}</label>
      <textarea
        id="json-paste-input"
        bind:value={pasteInput}
        rows="6"
        class="paste-textarea"
        placeholder={t('{\n  "id": "42",\n  "name": "ACME Corp",\n  "status": "active"\n}')}
        data-testid="json-paste-builder-textarea"></textarea>
      {#if parseError}
        <div class="form-error" role="alert" data-testid="json-paste-builder-error">{parseError}</div>
      {/if}
      <button
        type="button"
        class="btn btn-primary btn-sm"
        onclick={handleParse}
        data-testid="json-paste-builder-analyze-button"
      >
        {t('Analyze and make it variable')}
      </button>
    </div>
  {:else}
    <div class="paste-header">
      <span class="field-hint"
        >{isArrayRoot
          ? tCount(
              fields.length,
              'Array of {0} detected field: choose the source of each value',
              'Array of {0} detected fields: choose the source of each value',
            )
          : tCount(
              fields.length,
              '{0} field detected: choose the source of each value',
              '{0} fields detected: choose the source of each value',
            )}</span
      >
      <button
        type="button"
        class="btn btn-outline btn-sm"
        onclick={() => {
          parsed = false;
          pasteInput = '';
        }}
        data-testid="json-paste-builder-reset-button"
      >
        {t('Paste another JSON')}
      </button>
    </div>

    {#snippet renderFields(fieldList, path, depth)}
      {#each fieldList as field, idx}
        {@const currentPath = [...path, idx]}
        {@const testPath = currentPath.join('-')}
        {@const isObject = field.fieldType === 'object'}
        {@const collapsed = isObject && isCollapsed(testPath)}
        <div class="paste-field" style:margin-left="{depth * 1.25}rem">
          <div class="paste-field-main">
            {#if isObject}
              <button
                type="button"
                class="btn-icon btn-icon-xs collapse-toggle"
                onclick={() => toggleCollapse(testPath)}
                aria-expanded={!collapsed}
                aria-controls="json-paste-builder-children-{testPath}"
                aria-label={collapsed
                  ? t('Expand {0}', field.key || t('this field'))
                  : t('Collapse {0}', field.key || t('this field'))}
                title={collapsed ? t('Expand') : t('Collapse')}
                data-testid="json-paste-builder-collapse-button-{testPath}">{collapsed ? '▶' : '▼'}</button
              >
            {/if}
            <span class="paste-key">{field.key}</span>

            {#if isObject}
              <span class="paste-type-badge">{t('object')}</span>
              {#if collapsed}
                <span class="collapsed-indicator" data-testid="json-paste-builder-collapsed-indicator-{testPath}"
                  >{tCount((field.children || []).length, '({0} hidden item)', '({0} hidden items)')}</span
                >
              {/if}
            {/if}
          </div>

          {#if isObject}
            <div class="nested-block" id="json-paste-builder-children-{testPath}" hidden={collapsed}>
              {@render renderFields(field.children, [...currentPath, 'children'], depth + 1)}
            </div>
          {:else if field.fieldType === 'array-values' || field.fieldType === 'array-objects'}
            <span class="paste-type-badge">{t('array')}</span>
          {:else}
            <div class="paste-controls">
              <select
                value={field.source}
                onchange={(e) => updateField(currentPath, 'source', e.target.value)}
                aria-label={t('Source for {0}', field.key)}
                data-testid="json-paste-builder-source-select-{currentPath.join('-')}"
              >
                {#each valueSources as vs}
                  <option value={vs.value}>{vs.label}</option>
                {/each}
              </select>

              {#if field.source === 'fake'}
                <select
                  value={field.value}
                  onchange={(e) => updateField(currentPath, 'value', e.target.value)}
                  aria-label={t('Kind of fake data')}
                  data-testid="json-paste-builder-fake-select-{currentPath.join('-')}"
                >
                  {#each fakeOptions as fo}
                    <option value={fo}>{fo}</option>
                  {/each}
                </select>
              {:else if needsValueInput(field.source)}
                <input
                  type="text"
                  class="paste-value"
                  value={field.value}
                  oninput={(e) => updateField(currentPath, 'value', e.target.value)}
                  placeholder={valuePlaceholder(field.source)}
                  aria-label={t('Value for {0}', field.key)}
                  data-testid="json-paste-builder-value-input-{currentPath.join('-')}"
                />
              {/if}

              {#if field.source !== 'fixed'}
                <input
                  type="text"
                  class="pipe-input"
                  value={field.pipe || ''}
                  oninput={(e) => updateField(currentPath, 'pipe', e.target.value)}
                  placeholder={t('e.g. {0}', 'first(9) | upper')}
                  aria-label={t('Transformation pipe for {0}', field.key)}
                  list="dl-paste-pipes"
                  autocomplete="off"
                  data-testid="json-paste-builder-pipe-input-{currentPath.join('-')}"
                />
              {/if}

              {#if field.source === 'fixed'}
                <span class="paste-preview-fixed" translate="no">{field.value}</span>
              {:else}
                <span class="paste-preview-var" translate="no">{sharedBuildExpr(field)}</span>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    {/snippet}

    {@render renderFields(fields, [], 0)}

    <datalist id="dl-paste-pipes">
      {#each pipeOptions as p}<option value={p.value}>{p.label}</option>{/each}
    </datalist>
  {/if}
</div>

<style>
  .paste-builder {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .paste-zone {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .paste-zone label {
    font-weight: var(--weight-strong);
    font-size: var(--text-m);
  }
  .paste-textarea {
    width: 100%;
    font-family: var(--font-code);
    font-size: var(--text-s);
    padding: var(--space-2);
    border: var(--line-thick) dashed var(--color-control);
    border-radius: var(--radius-m);
    background: var(--color-bg);
    color: var(--color-text);
    resize: vertical;
    min-height: 8rem;
  }
  .paste-textarea:focus {
    border-color: var(--color-primary);
    border-style: solid;
  }

  .paste-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .paste-field {
    padding: var(--space-1-5) 0;
    border-bottom: var(--line-thin) solid var(--color-border);
  }
  .paste-field:last-child {
    border-bottom: none;
  }

  .paste-field-main {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    align-items: flex-start;
  }

  .paste-key {
    font-weight: var(--weight-heavy);
    font-size: var(--text-m);
    color: var(--color-primary);
    font-family: var(--font-code);
  }

  .paste-type-badge {
    display: inline-block;
    font-size: var(--text-xs);
    font-weight: var(--weight-strong);
    color: var(--color-text-muted);
    background: var(--color-bg);
    padding: var(--space-0-5) var(--space-1-5);
    border-radius: var(--radius-s);
    width: fit-content;
  }

  .collapse-toggle {
    flex-shrink: 0;
  }

  .nested-block {
    margin-top: var(--space-1);
    padding-left: var(--space-3);
    border-left: var(--line-thick) solid var(--color-primary);
  }

  .paste-controls {
    display: flex;
    gap: var(--space-1-5);
    align-items: center;
    flex-wrap: wrap;
  }
  .paste-controls select {
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
    background: var(--color-surface);
    color: var(--color-text);
  }
  .paste-value {
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
    background: var(--color-surface);
    color: var(--color-text);
    min-width: 8rem;
    flex: 1;
  }
  .pipe-input {
    min-width: 8rem;
    max-width: 14rem;
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
    font-family: var(--font-code);
    color: var(--color-primary);
  }

  .paste-preview-fixed {
    font-size: var(--text-s);
    color: var(--color-text-muted);
    font-style: italic;
  }
  .paste-preview-var {
    font-size: var(--text-s);
    color: var(--color-success);
    font-family: var(--font-code);
    font-weight: var(--weight-strong);
  }
</style>
