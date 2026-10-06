<script>
  import { fieldsToTemplate, buildExpr as sharedBuildExpr, templateToPreview } from '../tpl-utils.js';
  import { removeFolds, swapFolds } from '../fold-paths.js';
  import { t, tCount } from '../i18n.svelte.js';

  // arrayRoot: the sample pasted by example was an array; these fields then shape its item.
  let { fields = [], arrayRoot = false, onUpdate = () => {} } = $props();

  const fieldTypes = [
    {
      value: 'value',
      get label() {
        return t('Value');
      },
    },
    {
      value: 'object',
      get label() {
        return t('Object');
      },
    },
    {
      value: 'array-values',
      get label() {
        return t('Array');
      },
    },
    {
      value: 'array-objects',
      get label() {
        return t('Array of objects');
      },
    },
  ];

  const valueSources = [
    {
      value: 'fixed',
      get label() {
        return t('Fixed value');
      },
    },
    {
      value: 'path',
      get label() {
        return t('URL parameter {param}');
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
        return t('Echo of the body (JSON pointer)');
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
        return t('Sequence counter');
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

  function deepClone(obj) {
    return JSON.parse(JSON.stringify(obj));
  }

  function getByPath(root, path) {
    let current = root;
    for (const key of path) current = current[key];
    return current;
  }

  function getByPathSafe(root, path) {
    let current = root;
    for (const key of path) {
      if (current == null) return undefined;
      current = current[key];
    }
    return current;
  }

  // Breadcrumb navigation: focusPath points to an array of fields, the root ([]) or [...idx, 'children' | 'template'].
  // It shows one level at a time, as a help on top of the indented tree, and goes back to the root when the path no
  // longer exists (effect below).
  let focusPath = $state([]);

  $effect(() => {
    if (focusPath.length > 0 && getByPathSafe(fields, focusPath) === undefined) {
      focusPath = [];
    }
  });

  let focusedFields = $derived(getByPathSafe(fields, focusPath) ?? []);

  // Folding of nested fields (object, array), as in an IDE: an in-memory Set, local to this editing session and never
  // stored. Keyed by the positional test path (the one of the data-testid attributes), so moving or deleting a field
  // carries the folds along (fold-paths.js). Everything starts unfolded.
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

  function nestedCount(field, ft) {
    if (ft === 'object') return (field.children || []).length;
    if (ft === 'array-values') return (field.items || []).length;
    if (ft === 'array-objects') return (field.template || []).length;
    return 0;
  }

  function breadcrumbTrail(path) {
    const trail = [
      {
        get label() {
          return t('root');
        },
        path: [],
      },
    ];
    let current = fields;
    for (let i = 0; i < path.length; i += 2) {
      const idx = path[i];
      const prop = path[i + 1];
      const field = current?.[idx];
      if (!field) break;
      trail.push({ label: field.key?.trim() || `#${idx + 1}`, path: path.slice(0, i + 2) });
      current = field[prop];
    }
    return trail;
  }

  let breadcrumb = $derived(breadcrumbTrail(focusPath));

  function mutate(fn) {
    const clone = deepClone(fields);
    fn(clone);
    fields = clone;
    emit();
  }

  const pipeOptions = [
    {
      value: '',
      get label() {
        return t('(none)');
      },
    },
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

  function newValueField() {
    return { key: '', fieldType: 'value', source: 'fixed', value: '', pipe: '', asNumber: false };
  }

  function addFieldAt(path) {
    mutate((root) => getByPath(root, path).push(newValueField()));
  }

  function addArrayItem(path) {
    mutate((root) => getByPath(root, path).push({ source: 'fixed', value: '', asNumber: false }));
  }

  function removeAt(path, idx) {
    const length = getByPath(fields, path).length;
    mutate((root) => getByPath(root, path).splice(idx, 1));
    collapsedPaths = removeFolds(collapsedPaths, path, idx, length);
  }

  function moveAt(path, idx, dir) {
    const t = idx + dir;
    if (t < 0 || t >= getByPath(fields, path).length) return;
    mutate((root) => {
      const arr = getByPath(root, path);
      [arr[idx], arr[t]] = [arr[t], arr[idx]];
    });
    collapsedPaths = swapFolds(collapsedPaths, path, idx, t);
  }

  function updateProp(path, idx, prop, val) {
    mutate((root) => {
      const arr = getByPath(root, path);
      arr[idx] = { ...arr[idx], [prop]: val };
      if (prop === 'source') {
        if (['uuid', 'now_ms', 'now_iso', 'seq'].includes(val)) arr[idx].value = '';
        if (val === 'fake') arr[idx].value = 'CompanyName';
      }
    });
  }

  function changeFieldType(path, idx, newType) {
    mutate((root) => {
      const arr = getByPath(root, path);
      const old = arr[idx];
      const key = old.key || '';
      if (newType === 'value') {
        arr[idx] = { key, fieldType: 'value', source: 'fixed', value: '', asNumber: false };
      } else if (newType === 'object') {
        arr[idx] = { key, fieldType: 'object', children: [] };
      } else if (newType === 'array-values') {
        arr[idx] = { key, fieldType: 'array-values', items: [] };
      } else if (newType === 'array-objects') {
        arr[idx] = { key, fieldType: 'array-objects', template: [] };
      }
    });
  }

  function emit() {
    onUpdate(fields);
  }

  // 'script' needs a value input: it names the key of the script result to use (buildExpr in tpl-utils.js produces
  // `{{script.<value>}}`, or `{{script}}` when the value is empty).
  function needsValueInput(source) {
    return ['fixed', 'path', 'query', 'header', 'body', 'script'].includes(source);
  }

  function fieldPlaceholder(source) {
    switch (source) {
      case 'fixed':
        return t('e.g. active');
      case 'path':
        return t('e.g. id');
      case 'query':
        return t('e.g. page');
      case 'header':
        return t('e.g. x-request-id');
      case 'body':
        return t('e.g. /user/name');
      // A script result is read one flat key deep ({{script.field}}, never {{script.object.field}}): an object nested
      // under a key (`#{ city: pick, id: uuid() }`) comes out whole, as JSON, not as fields of its own. The rule
      // tester shows the keys a script really returns.
      case 'script':
        return t('e.g. name (empty = the whole {{script}}; one level only: test the rule to see the keys)');
      default:
        return '';
    }
  }

  function buildExpr(f) {
    return sharedBuildExpr(f);
  }

  export function toTemplate() {
    const object = fieldsToTemplate(fields);
    return arrayRoot ? `[${object}]` : object;
  }

  function previewJson() {
    try {
      return templateToPreview(toTemplate());
    } catch {
      return t('(error)');
    }
  }
</script>

<div class="json-builder" aria-label={t('JSON response builder')}>
  <div class="builder-header">
    <strong>{t('Fields of the JSON response')}</strong>
    <span class="field-hint">{t('Build the JSON structure: values, nested objects, arrays.')}</span>
    {#if arrayRoot}
      <span class="field-hint" data-testid="json-builder-array-root-hint"
        >{t('The response is an array: these fields shape its item.')}</span
      >
    {/if}
  </div>

  {#if fields.length === 0}
    <p class="empty-msg">{t('No field. Click “+ Add a field” to start.')}</p>
  {/if}

  {#snippet renderValueControls(field, path, idx)}
    {@const testPath = [...path, idx].join('-')}
    <select
      value={field.source}
      onchange={(e) => updateProp(path, idx, 'source', e.target.value)}
      aria-label={t('Source of the value')}
      data-testid="json-builder-source-select-{testPath}"
    >
      {#each valueSources as vs}
        <option value={vs.value}>{vs.label}</option>
      {/each}
    </select>
    {#if field.source === 'fake'}
      <select
        value={field.value}
        onchange={(e) => updateProp(path, idx, 'value', e.target.value)}
        aria-label={t('Fake data kind')}
        data-testid="json-builder-fake-select-{testPath}"
      >
        {#each fakeOptions as fo}
          <option value={fo}>{fo}</option>
        {/each}
      </select>
    {:else if needsValueInput(field.source)}
      <input
        type="text"
        class="value-input"
        value={field.value}
        oninput={(e) => updateProp(path, idx, 'value', e.target.value)}
        placeholder={fieldPlaceholder(field.source)}
        aria-label={t('Value')}
        data-testid="json-builder-value-input-{testPath}"
      />
    {/if}
    {#if field.source !== 'fixed'}
      <input
        type="text"
        class="pipe-input"
        value={field.pipe || ''}
        oninput={(e) => updateProp(path, idx, 'pipe', e.target.value)}
        placeholder={t('e.g. {0}', 'first(9) | upper')}
        aria-label={t('Transformation pipe')}
        list="dl-pipes"
        autocomplete="off"
        data-testid="json-builder-pipe-input-{testPath}"
      />
    {/if}
    <label class="number-toggle" title={t('Render without quotes (JSON number)')}>
      <input
        type="checkbox"
        checked={field.asNumber}
        onchange={(e) => updateProp(path, idx, 'asNumber', e.target.checked)}
        data-testid="json-builder-asnumber-checkbox-{testPath}"
      />
      <span class="number-label">#</span>
    </label>
  {/snippet}

  {#snippet renderFields(fieldList, path, depth)}
    {#each fieldList as field, idx}
      {@const ft = field.fieldType || 'value'}
      {@const testPath = [...path, idx].join('-')}
      {@const hasNested = ft === 'object' || ft === 'array-values' || ft === 'array-objects'}
      {@const collapsed = hasNested && isCollapsed(testPath)}
      <div class="field-row" style:margin-left="{depth * 1.25}rem">
        <div class="field-main">
          {#if hasNested}
            <button
              type="button"
              class="btn-icon btn-icon-xs collapse-toggle"
              onclick={() => toggleCollapse(testPath)}
              aria-expanded={!collapsed}
              aria-controls="json-builder-children-{testPath}"
              aria-label={collapsed
                ? t('Expand {0}', field.key || t('this field'))
                : t('Collapse {0}', field.key || t('this field'))}
              title={collapsed ? t('Expand') : t('Collapse')}
              data-testid="json-builder-collapse-button-{testPath}">{collapsed ? '▶' : '▼'}</button
            >
          {/if}
          <input
            type="text"
            class="key-input"
            value={field.key}
            oninput={(e) => updateProp(path, idx, 'key', e.target.value)}
            placeholder={t('key')}
            aria-label={t('Key name')}
            data-testid="json-builder-key-input-{testPath}"
          />
          <select
            class="type-select"
            value={ft}
            onchange={(e) => changeFieldType(path, idx, e.target.value)}
            aria-label={t('Field type')}
            data-testid="json-builder-type-select-{testPath}"
          >
            {#each fieldTypes as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
          {#if ft === 'value'}
            {@render renderValueControls(field, path, idx)}
          {/if}
          {#if collapsed}
            <span class="collapsed-indicator" data-testid="json-builder-collapsed-indicator-{testPath}"
              >{tCount(nestedCount(field, ft), '({0} hidden item)', '({0} hidden items)')}</span
            >
          {/if}
          <div class="field-actions">
            {#if ft === 'object' || ft === 'array-objects'}
              <button
                type="button"
                class="btn-icon btn-icon-xs"
                onclick={() => (focusPath = [...path, idx, ft === 'object' ? 'children' : 'template'])}
                aria-label={t('Go into {0}', field.key || t('this field'))}
                title={t('Go into this field')}
                data-testid="json-builder-navigate-button-{testPath}">&#8594;</button
              >
            {/if}
            <button
              type="button"
              class="btn-icon btn-icon-xs"
              onclick={() => moveAt(path, idx, -1)}
              disabled={idx === 0}
              aria-label={t('Move up')}
              title={t('Move up')}
              data-testid="json-builder-moveup-button-{testPath}">&#9650;</button
            >
            <button
              type="button"
              class="btn-icon btn-icon-xs"
              onclick={() => moveAt(path, idx, 1)}
              disabled={idx === fieldList.length - 1}
              aria-label={t('Move down')}
              title={t('Move down')}
              data-testid="json-builder-movedown-button-{testPath}">&#9660;</button
            >
            <button
              type="button"
              class="btn-icon btn-icon-xs btn-delete"
              onclick={() => removeAt(path, idx)}
              aria-label={t('Delete the field {0}', field.key || idx + 1)}
              data-testid="json-builder-delete-button-{testPath}">&#10005;</button
            >
          </div>
        </div>

        {#if ft === 'object'}
          <div class="nested-block" id="json-builder-children-{testPath}" hidden={collapsed}>
            {@render renderFields(field.children || [], [...path, idx, 'children'], depth + 1)}
            <button
              type="button"
              class="btn btn-xs btn-outline"
              onclick={() => addFieldAt([...path, idx, 'children'])}
              data-testid="json-builder-add-subfield-button-{testPath}">{t('+ Sub-field')}</button
            >
          </div>
        {:else if ft === 'array-values'}
          <div class="nested-block" id="json-builder-children-{testPath}" hidden={collapsed}>
            {#each field.items || [] as item, iidx}
              <div class="array-item">
                <span class="item-index">{iidx + 1}</span>
                {@render renderValueControls(item, [...path, idx, 'items'], iidx)}
                <button
                  type="button"
                  class="btn-icon btn-icon-xs btn-delete"
                  onclick={() => removeAt([...path, idx, 'items'], iidx)}
                  aria-label={t('Delete the item {0}', iidx + 1)}
                  data-testid="json-builder-delete-item-button-{testPath}-{iidx}">&#10005;</button
                >
              </div>
            {/each}
            <button
              type="button"
              class="btn btn-xs btn-outline"
              onclick={() => addArrayItem([...path, idx, 'items'])}
              data-testid="json-builder-add-item-button-{testPath}">{t('+ Item')}</button
            >
          </div>
        {:else if ft === 'array-objects'}
          <div class="nested-block" id="json-builder-children-{testPath}" hidden={collapsed}>
            <span class="nested-hint">{t('Shape of an array item:')}</span>
            {@render renderFields(field.template || [], [...path, idx, 'template'], depth + 1)}
            <button
              type="button"
              class="btn btn-xs btn-outline"
              onclick={() => addFieldAt([...path, idx, 'template'])}
              data-testid="json-builder-add-template-field-button-{testPath}">{t('+ Field')}</button
            >
          </div>
        {/if}
      </div>
    {/each}
  {/snippet}

  {#if focusPath.length > 0}
    <nav class="data-breadcrumb" aria-label={t('Data path')}>
      <ol>
        {#each breadcrumb as segment, i}
          <li aria-current={i === breadcrumb.length - 1 ? 'page' : undefined}>
            {#if i === breadcrumb.length - 1}
              <span>{segment.label}</span>
            {:else}
              <button
                type="button"
                class="breadcrumb-link"
                onclick={() => (focusPath = segment.path)}
                data-testid="json-builder-breadcrumb-link-{i}">{segment.label}</button
              >
            {/if}
          </li>
        {/each}
      </ol>
    </nav>
  {/if}

  {@render renderFields(focusedFields, focusPath, 0)}

  <button
    type="button"
    class="btn btn-sm btn-outline"
    onclick={() => addFieldAt(focusPath)}
    data-testid="json-builder-add-field-button">{t('+ Add a field')}</button
  >

  <datalist id="dl-pipes">
    {#each pipeOptions.filter((p) => p.value) as p}<option value={p.value}>{p.label}</option>{/each}
  </datalist>

  {#if fields.length > 0}
    <details class="preview-section">
      <summary>{t('Preview of the generated template')}</summary>
      <code class="preview-code">{toTemplate()}</code>
    </details>
    <details class="preview-section">
      <summary>{t('Readable JSON preview')}</summary>
      <code class="preview-code preview-readable">{previewJson()}</code>
    </details>
  {/if}
</div>

<style>
  .json-builder {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .builder-header strong {
    font-size: var(--text-m);
  }
  .empty-msg {
    color: var(--color-text-muted);
    font-style: italic;
    font-size: var(--text-m);
    margin: var(--space-1) 0;
  }

  .field-row {
    padding: var(--space-1-5);
    background: var(--color-bg);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    margin-bottom: var(--space-1);
  }

  .field-main {
    display: flex;
    gap: var(--space-1-5);
    align-items: center;
    flex-wrap: wrap;
  }

  .key-input {
    width: 7rem;
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
    font-weight: var(--weight-strong);
  }
  .type-select {
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
    min-width: 5rem;
    background: var(--color-surface);
  }
  .value-input {
    flex: 1;
    min-width: 6rem;
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
  }
  select {
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
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
  .number-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    cursor: pointer;
  }
  .number-toggle input {
    width: 1rem;
    height: 1rem;
  }
  .number-label {
    font-size: var(--text-xs);
    font-weight: var(--weight-heavy);
    color: var(--color-text-muted);
  }

  .field-actions {
    display: flex;
    gap: var(--space-1);
    margin-left: auto;
  }

  .nested-block {
    margin-top: var(--space-1-5);
    padding-left: var(--space-3);
    border-left: var(--line-thick) solid var(--color-primary);
  }
  .nested-hint {
    font-size: var(--text-s);
    color: var(--color-text-muted);
    font-style: italic;
    display: block;
    margin-bottom: var(--space-1);
  }

  .collapse-toggle {
    flex-shrink: 0;
  }

  .array-item {
    display: flex;
    gap: var(--space-1-5);
    align-items: center;
    flex-wrap: wrap;
    padding: var(--space-1) 0;
  }
  .item-index {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.25rem;
    height: 1.25rem;
    border-radius: var(--radius-round);
    background: var(--color-text-muted);
    color: var(--color-surface);
    font-size: var(--text-xs);
    font-weight: var(--weight-heavy);
    flex-shrink: 0;
  }

  .breadcrumb-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--color-primary);
    cursor: pointer;
    font: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .breadcrumb-link:hover {
    color: var(--color-primary-hover);
  }

  .preview-section {
    margin-top: var(--space-1-5);
  }
  .preview-section summary {
    font-size: var(--text-s);
    cursor: pointer;
    color: var(--color-text-muted);
  }
  .preview-readable {
    color: var(--color-primary);
  }
</style>
