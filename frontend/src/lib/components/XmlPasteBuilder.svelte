<script>
  // The by-example XML view, counterpart of JsonPasteBuilder.svelte: the pasted sample goes through exampleXmlToFields,
  // then each value gets a source. No node is renamed, added or removed here; the detailed view does that (see
  // docs/en/responses-and-templates.md). Two additions over the JSON view:
  //  - breadcrumb navigation (focusPath) and fold chevrons (collapsedPaths, hidden with the `hidden` attribute as in
  //    XmlResponseBuilder.svelte): pasted XML, a SOAP envelope typically, is nested much deeper than a REST JSON
  //    payload, and a fully expanded tree would be unreadable;
  //  - XML attributes, which JSON does not have: every node, root included, may carry attributes, each with its own
  //    source, as for text content. Attribute names stay read-only, like JSON keys in this view.
  //
  // Namespace prefixes ("soap:Envelope") and xmlns / xmlns:* attributes are kept as literal text, never resolved (see
  // exampleXmlToFields in tpl-utils.js).
  import { untrack } from 'svelte';
  import { buildExpr as sharedBuildExpr, xmlFieldsToTemplate, exampleXmlToFields } from '../tpl-utils.js';
  import Sentence from './Sentence.svelte';
  import { t, tCount } from '../i18n.svelte.js';

  // startParsed, rootTag and rootAttributes: the parent holds them (rebuilt from a saved template by
  // templateToXmlFields in RuleResponseSection.svelte, or kept from the other view). Read once, when the component is
  // created (untrack), as in JsonPasteBuilder.svelte; each change of the root goes back through onRootChange, since
  // this component is unmounted whenever the response section is folded or the view changes.
  let {
    fields = [],
    startParsed = false,
    rootTag: initialRootTag = 'response',
    rootAttributes: initialRootAttributes = [],
    onUpdate = () => {},
    onRootChange = () => {},
  } = $props();

  let pasteInput = $state('');
  let parseError = $state('');
  let parsed = $state(untrack(() => startParsed));
  let rootTag = $state(untrack(() => initialRootTag));
  let rootAttributes = $state(untrack(() => initialRootAttributes));

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
        return t('Echo of the body (JSON pointer)');
      },
    },
    {
      value: 'xpath',
      get label() {
        return t('XPath (XML/SOAP)');
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

  // Pipe suggestions for the content of value nodes, the same as XmlResponseBuilder.svelte. Attributes take a source
  // but no pipe.
  const pipeOptions = [
    { value: 'lower', label: 'lower' },
    { value: 'upper', label: 'upper' },
    { value: 'capitalize', label: 'capitalize' },
    { value: 'first(N)', label: 'first(N)' },
    { value: 'last(N)', label: 'last(N)' },
    { value: 'substr(start,len)', label: 'substr(start,len)' },
    { value: 'default("val")', label: 'default("val")' },
    { value: 'replace("a","b")', label: 'replace("a","b")' },
    { value: 'prepend("prefix")', label: 'prepend("prefix")' },
    { value: 'append("suffix")', label: 'append("suffix")' },
    { value: 'length', label: 'length' },
    { value: 'trim', label: 'trim' },
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

  // focusPath only enters 'children': a JSON field also nests through 'items' and 'template', an XML node one way only.
  // It goes back to the root when it no longer points to anything (the same guard as JsonResponseBuilder.svelte).
  let focusPath = $state([]);
  let collapsedPaths = $state(new Set());

  $effect(() => {
    if (focusPath.length > 0 && getByPathSafe(fields, focusPath) === undefined) {
      focusPath = [];
    }
  });

  let focusedFields = $derived(getByPathSafe(fields, focusPath) ?? []);

  function isCollapsed(testPath) {
    return collapsedPaths.has(testPath);
  }

  function toggleCollapse(testPath) {
    const next = new Set(collapsedPaths);
    if (next.has(testPath)) next.delete(testPath);
    else next.add(testPath);
    collapsedPaths = next;
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
      const field = current?.[idx];
      if (!field) break;
      trail.push({ label: field.tag?.trim() || `#${idx + 1}`, path: path.slice(0, i + 2) });
      current = field.children;
    }
    return trail;
  }

  let breadcrumb = $derived(breadcrumbTrail(focusPath));

  function handleParse() {
    parseError = '';
    const text = pasteInput.trim();
    if (!text) {
      parseError = t('Paste valid XML.');
      return;
    }
    try {
      const result = exampleXmlToFields(text);
      rootTag = result.rootTag;
      rootAttributes = result.rootAttributes;
      fields = result.fields;
      focusPath = [];
      collapsedPaths = new Set();
      parsed = true;
      onRootChange({ rootTag, rootAttributes });
      emit();
    } catch (e) {
      parseError = e.message;
    }
  }

  function resetPaste() {
    parsed = false;
    pasteInput = '';
    parseError = '';
    focusPath = [];
    collapsedPaths = new Set();
  }

  function mutate(fn) {
    const clone = deepClone(fields);
    fn(clone);
    fields = clone;
    emit();
  }

  function applySourceDefaults(node, prop, val) {
    if (prop === 'source' && val === 'fake') node.value = 'CompanyName';
    if (prop === 'source' && ['uuid', 'now_ms', 'now_iso', 'seq'].includes(val)) node.value = '';
  }

  function updateNodeProp(path, idx, prop, val) {
    mutate((root) => {
      const arr = getByPath(root, path);
      const node = { ...arr[idx], [prop]: val };
      applySourceDefaults(node, prop, val);
      arr[idx] = node;
    });
  }

  function updateAttrProp(path, idx, attrIdx, prop, val) {
    mutate((root) => {
      const arr = getByPath(root, path);
      const node = { ...arr[idx] };
      const attrs = [...(node.attributes || [])];
      const attr = { ...attrs[attrIdx], [prop]: val };
      applySourceDefaults(attr, prop, val);
      attrs[attrIdx] = attr;
      node.attributes = attrs;
      arr[idx] = node;
    });
  }

  function updateRootAttrProp(attrIdx, prop, val) {
    const clone = deepClone(rootAttributes);
    const attr = { ...clone[attrIdx], [prop]: val };
    applySourceDefaults(attr, prop, val);
    clone[attrIdx] = attr;
    rootAttributes = clone;
    onRootChange({ rootTag, rootAttributes });
    emit();
  }

  function emit() {
    onUpdate(fields);
  }

  function needsValueInput(src) {
    return ['fixed', 'path', 'query', 'header', 'body', 'xpath', 'script'].includes(src);
  }

  function valuePlaceholder(src) {
    if (src === 'fixed') return t('fixed value');
    if (src === 'xpath') return t('e.g. Envelope/Body/search/Id');
    // As in XmlResponseBuilder.svelte: a script result is read one flat key deep ({{script.field}}).
    if (src === 'script')
      return t('e.g. name (empty = the whole {{script}}; one level only: test the rule to see the keys)');
    return t('parameter name');
  }

  function buildExpr(f) {
    return sharedBuildExpr(f);
  }

  export function toTemplate() {
    return xmlFieldsToTemplate(fields, rootTag, rootAttributes);
  }
</script>

<div class="paste-builder" aria-label={t('XML builder by example')}>
  {#if !parsed}
    <div class="paste-zone">
      <label for="xml-paste-input">{t('Paste an example of the XML response')}</label>
      <textarea
        id="xml-paste-input"
        bind:value={pasteInput}
        rows="6"
        class="paste-textarea"
        placeholder={t('<response>\n  <id>42</id>\n  <name>ACME Corp</name>\n</response>')}
        data-testid="xml-paste-builder-textarea"></textarea>
      {#if parseError}
        <div class="form-error" role="alert" data-testid="xml-paste-builder-error">{parseError}</div>
      {/if}
      <button
        type="button"
        class="btn btn-primary btn-sm"
        onclick={handleParse}
        data-testid="xml-paste-builder-analyze-button"
      >
        {t('Analyze and make it variable')}
      </button>
    </div>
  {:else}
    <div class="paste-header">
      <span class="field-hint"
        ><Sentence
          text={tCount(
            fields.length,
            'Root {1}, {0} root node detected: choose the source of each value',
            'Root {1}, {0} root nodes detected: choose the source of each value',
          )}
          codes={['', `<${rootTag}>`]}
        /></span
      >
      <button
        type="button"
        class="btn btn-outline btn-sm"
        onclick={resetPaste}
        data-testid="xml-paste-builder-reset-button"
      >
        {t('Paste another XML')}
      </button>
    </div>

    {#snippet renderAttributes(attributes, testPathPrefix, onAttrUpdate)}
      {#if attributes && attributes.length > 0}
        <div class="paste-attrs">
          <span class="paste-attrs-label">{t('Attributes:')}</span>
          {#each attributes as attr, aidx}
            <div class="paste-attr-row">
              <span class="paste-attr-name">@{attr.name}</span>
              <select
                value={attr.source}
                onchange={(e) => onAttrUpdate(aidx, 'source', e.target.value)}
                aria-label={t('Source for the attribute {0} ({1})', attr.name, testPathPrefix)}
                data-testid="xml-paste-builder-attr-source-select-{testPathPrefix}-{aidx}"
              >
                {#each valueSources as vs}<option value={vs.value}>{vs.label}</option>{/each}
              </select>
              {#if attr.source === 'fake'}
                <select
                  value={attr.value}
                  onchange={(e) => onAttrUpdate(aidx, 'value', e.target.value)}
                  aria-label={t('Fake data kind for the attribute {0} ({1})', attr.name, testPathPrefix)}
                  data-testid="xml-paste-builder-attr-fake-select-{testPathPrefix}-{aidx}"
                >
                  {#each fakeOptions as fo}<option value={fo}>{fo}</option>{/each}
                </select>
              {:else if needsValueInput(attr.source)}
                <input
                  type="text"
                  class="paste-value"
                  value={attr.value}
                  oninput={(e) => onAttrUpdate(aidx, 'value', e.target.value)}
                  placeholder={valuePlaceholder(attr.source)}
                  aria-label={t('Value for the attribute {0} ({1})', attr.name, testPathPrefix)}
                  data-testid="xml-paste-builder-attr-value-input-{testPathPrefix}-{aidx}"
                />
              {/if}
              {#if attr.source === 'fixed'}
                <span class="paste-preview-fixed" translate="no">{attr.value}</span>
              {:else}
                <span class="paste-preview-var" translate="no">{buildExpr(attr)}</span>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {/snippet}

    {@render renderAttributes(rootAttributes, 'root', (aidx, prop, val) => updateRootAttrProp(aidx, prop, val))}

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
                  data-testid="xml-paste-builder-breadcrumb-link-{i}">{segment.label}</button
                >
              {/if}
            </li>
          {/each}
        </ol>
      </nav>
    {/if}

    {#snippet renderNodes(nodeList, path, depth)}
      {#each nodeList as field, idx}
        {@const nt = field.nodeType || 'value'}
        {@const testPath = [...path, idx].join('-')}
        {@const hasChildren = nt === 'parent'}
        {@const collapsed = hasChildren && isCollapsed(testPath)}
        <div class="paste-field" style:margin-left="{depth * 1.25}rem">
          <div class="paste-field-main">
            {#if hasChildren}
              <button
                type="button"
                class="btn-icon btn-icon-xs collapse-toggle"
                onclick={() => toggleCollapse(testPath)}
                aria-expanded={!collapsed}
                aria-controls="xml-paste-builder-children-{testPath}"
                aria-label={collapsed
                  ? t('Expand {0}', field.tag || t('this node'))
                  : t('Collapse {0}', field.tag || t('this node'))}
                title={collapsed ? t('Expand') : t('Collapse')}
                data-testid="xml-paste-builder-collapse-button-{testPath}">{collapsed ? '▶' : '▼'}</button
              >
            {/if}
            <span class="paste-key">{field.tag}</span>
            {#if hasChildren}
              <span class="paste-type-badge">{t('parent node ({0})', (field.children || []).length)}</span>
              {#if collapsed}
                <span class="collapsed-indicator" data-testid="xml-paste-builder-collapsed-indicator-{testPath}"
                  >{tCount((field.children || []).length, '({0} hidden item)', '({0} hidden items)')}</span
                >
              {/if}
              <button
                type="button"
                class="btn-icon btn-icon-xs"
                onclick={() => (focusPath = [...path, idx, 'children'])}
                aria-label={t('Go into {0}', field.tag || t('this node'))}
                title={t('Go into this node')}
                data-testid="xml-paste-builder-navigate-button-{testPath}">&#8594;</button
              >
            {:else}
              <div class="paste-controls">
                <select
                  value={field.source}
                  onchange={(e) => updateNodeProp(path, idx, 'source', e.target.value)}
                  aria-label={t('Source for {0}', field.tag)}
                  data-testid="xml-paste-builder-source-select-{testPath}"
                >
                  {#each valueSources as vs}<option value={vs.value}>{vs.label}</option>{/each}
                </select>
                {#if field.source === 'fake'}
                  <select
                    value={field.value}
                    onchange={(e) => updateNodeProp(path, idx, 'value', e.target.value)}
                    aria-label={t('Fake data kind for {0}', field.tag)}
                    data-testid="xml-paste-builder-fake-select-{testPath}"
                  >
                    {#each fakeOptions as fo}<option value={fo}>{fo}</option>{/each}
                  </select>
                {:else if needsValueInput(field.source)}
                  <input
                    type="text"
                    class="paste-value"
                    value={field.value}
                    oninput={(e) => updateNodeProp(path, idx, 'value', e.target.value)}
                    placeholder={valuePlaceholder(field.source)}
                    aria-label={t('Value for {0}', field.tag)}
                    data-testid="xml-paste-builder-value-input-{testPath}"
                  />
                {/if}
                {#if field.source !== 'fixed'}
                  <input
                    type="text"
                    class="pipe-input"
                    value={field.pipe || ''}
                    oninput={(e) => updateNodeProp(path, idx, 'pipe', e.target.value)}
                    placeholder={t('e.g. {0}', 'lower | first(5)')}
                    aria-label={t('Transformation pipe for {0}', field.tag)}
                    list="dl-xml-paste-pipes"
                    autocomplete="off"
                    data-testid="xml-paste-builder-pipe-input-{testPath}"
                  />
                {/if}
                {#if field.source === 'fixed'}
                  <span class="paste-preview-fixed" translate="no">{field.value}</span>
                {:else}
                  <span class="paste-preview-var" translate="no">{buildExpr(field)}</span>
                {/if}
              </div>
            {/if}
          </div>

          {@render renderAttributes(field.attributes, testPath, (aidx, prop, val) =>
            updateAttrProp(path, idx, aidx, prop, val),
          )}

          {#if hasChildren}
            <div class="nested-block" id="xml-paste-builder-children-{testPath}" hidden={collapsed}>
              {@render renderNodes(field.children || [], [...path, idx, 'children'], depth + 1)}
            </div>
          {/if}
        </div>
      {/each}
    {/snippet}

    {@render renderNodes(focusedFields, focusPath, 0)}

    <datalist id="dl-xml-paste-pipes">
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

  .paste-attrs {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: var(--space-1) 0 var(--space-1-5);
    padding: var(--space-1-5) var(--space-2);
    background: var(--color-bg);
    border-radius: var(--radius-m);
    border: var(--line-thin) dashed var(--color-border);
  }
  .paste-attrs-label {
    font-size: var(--text-xs);
    font-weight: var(--weight-strong);
    color: var(--color-text-muted);
    text-transform: uppercase;
  }
  .paste-attr-row {
    display: flex;
    gap: var(--space-1-5);
    align-items: center;
    flex-wrap: wrap;
  }
  .paste-attr-name {
    font-family: var(--font-code);
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }
  .paste-attr-row select {
    padding: var(--space-1) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-s);
    background: var(--color-surface);
    color: var(--color-text);
  }

  .collapse-toggle {
    flex-shrink: 0;
  }

  .nested-block {
    margin-top: var(--space-1);
    padding-left: var(--space-3);
    border-left: var(--line-thick) solid var(--color-primary);
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
</style>
