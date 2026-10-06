<script>
  // The "Mocked response" fieldset of a rule: format selector (JSON, XML, text, advanced template, empty), headers and
  // body, and also the three script slots (pre_script, script, post_script) and the chaos settings. Those are fields of
  // the rule rather than of its response, but the form shows them in this fieldset.
  //
  // RuleForm.svelte keeps this component mounted whatever the rule's action: `visible` (true for a mock rule) only hides
  // the fieldset. Behind an {#if}, switching to proxy and back before saving would lose the headers, body, scripts and
  // chaos settings held here.
  //
  // RuleForm reads getPayload() and validate() through bind:this: getPayload() for every rule, validate() for a mock
  // rule only.
  import { untrack } from 'svelte';
  import JsonResponseBuilder from './JsonResponseBuilder.svelte';
  import JsonPasteBuilder from './JsonPasteBuilder.svelte';
  import XmlResponseBuilder from './XmlResponseBuilder.svelte';
  import XmlPasteBuilder from './XmlPasteBuilder.svelte';
  import RuleScriptSlot from './RuleScriptSlot.svelte';
  import ToggleSwitch from './ToggleSwitch.svelte';
  import {
    fieldsToTemplate,
    templateToFields,
    templateToXmlFields,
    validateTemplateAsJson,
    validateTemplateAsXml,
    xmlFieldsToTemplate,
  } from '../tpl-utils.js';
  import { validateScript as apiValidateScript } from '../api.js';
  import { RHAI_FUNCTIONS } from '../rhai-functions.js';
  import Sentence from './Sentence.svelte';
  import { t } from '../i18n.svelte.js';

  let { visible = true, initRule = null } = $props();

  const init = untrack(() => initRule);

  let status = $state(init?.response?.status ?? 200);
  let respHeaders = $state(init?.response?.headers ?? []);
  let fragments = $state(init?.response?.body ?? [{ type: 'Literal', value: '' }]);
  let chaosEnabled = $state(!!init?.response?.chaos);
  let chaos = $state(
    init?.response?.chaos ?? { delay_ms: 0, delay_min_ms: null, delay_max_ms: null, error_rate: 0, error_status: 500 },
  );

  let responseOpen = $state(true);

  // The pre-script and post-script slots start folded under "Advanced options", unless the rule already has one of them
  // (never hide what the user configured); the main script is never folded. Computed once, when the form opens. Their
  // code lives in this component, not in RuleScriptSlot, and folding only hides the panel (`hidden` attribute, not an
  // {#if} that would unmount it): nothing typed is lost.
  let advancedOpen = $state(!!init?.pre_script?.trim() || !!init?.post_script?.trim());

  const STRUCTURED_MODES = ['json-paste', 'json-guided', 'xml-paste', 'xml-guided'];

  // Reopening a rule shows the view that built it. The four structured views (json-paste, json-guided, xml-paste,
  // xml-guided) all save a single Template fragment, which tells neither them apart nor from a hand-written advanced
  // template: `Rule.response_mode` (src/models/mod.rs, read by the editor only) records the view. Without it (a rule
  // saved before that field existed), the view is guessed from the body's shape. A body that cannot be read back into
  // fields (invalid JSON or XML, a JSON array this editor did not write) opens on the advanced template instead of
  // failing.
  function computeInitialEditorState() {
    const body = init?.response?.body ?? [];
    const singleTemplate = body.length === 1 && body[0].type === 'Template' ? body[0].template : null;
    const singleLiteral = body.length === 1 && body[0].type === 'Literal' ? body[0].value : null;

    function fallbackMode() {
      if (!body.length) return 'json-paste';
      if (singleTemplate !== null) return 'advanced';
      if (singleLiteral !== null) return 'text';
      if (init?.response?.status === 204) return 'empty';
      return 'advanced';
    }

    let mode = init?.response_mode ?? fallbackMode();
    const structured = {};

    if (STRUCTURED_MODES.includes(mode)) {
      if (body.length === 0) {
        // An empty body (a new rule, or a rule whose response was never set) has nothing to restore: the view opens
        // empty. Treating it as a mismatch would open every new rule on the advanced template instead of JSON.
      } else if (singleTemplate === null) {
        // A body without the structured shape (configuration edited outside the UI): open the advanced template rather
        // than fail.
        mode = 'advanced';
      } else if (mode === 'json-paste' || mode === 'json-guided') {
        try {
          const r = readJsonTemplate(singleTemplate);
          structured.fields = r.fields;
          structured.arrayRoot = r.arrayRoot;
        } catch {
          mode = 'advanced';
        }
      } else if (mode === 'xml-paste' || mode === 'xml-guided') {
        try {
          const r = templateToXmlFields(singleTemplate);
          structured.fields = r.fields;
          structured.rootTag = r.rootTag;
          structured.rootAttributes = r.rootAttributes;
        } catch {
          mode = 'advanced';
        }
      }
    } else if (mode === 'text') {
      structured.textContent = singleLiteral ?? '';
    }

    return { mode, structured };
  }

  // A JSON template read back into fields. The by-example view saves an array sample as `[<item>]`, its item built by
  // fieldsToTemplate: without the brackets, the item must rebuild the template exactly, or the array is not one this
  // editor wrote (several items, hand-written text) and the error lets the caller keep the advanced template.
  function readJsonTemplate(tpl) {
    const text = tpl.trim();
    if (!text.startsWith('[')) return { fields: templateToFields(text), arrayRoot: false };
    try {
      const fields = templateToFields(text.slice(1, -1));
      if (`[${fieldsToTemplate(fields)}]` === text) return { fields, arrayRoot: true };
    } catch {
      /* reported below */
    }
    throw new TypeError(t('The JSON must be an object to be shown in the guided view.'));
  }

  const { mode: initialMode, structured: initialStructured } = computeInitialEditorState();

  let responseMode = $state(initialMode);

  // The by-example builders get `startParsed={jsonPasteFields.length > 0}` (or the XML one, in the markup below), never
  // a value derived from the mode: a new rule also starts in 'json-paste' or 'xml-paste' but has nothing to restore, so
  // it must show the paste area, not an empty list of fields. This holds after "Edit in detail" and back
  // (revealDetailMode, backToPasteMode), because Svelte mounts the builder again each time its {:else if} branch is
  // entered, and reads `startParsed` then.
  let jsonFields = $state(initialMode === 'json-guided' ? (initialStructured.fields ?? []) : []);
  let jsonPasteFields = $state(initialMode === 'json-paste' ? (initialStructured.fields ?? []) : []);
  // Both JSON views share the array root, and both XML views share the root tag and its attributes.
  let jsonArrayRoot = $state(initialStructured.arrayRoot ?? false);
  let xmlFields = $state(initialMode === 'xml-guided' ? (initialStructured.fields ?? []) : []);
  let xmlPasteFields = $state(initialMode === 'xml-paste' ? (initialStructured.fields ?? []) : []);
  let xmlRootTag = $state(initialStructured.rootTag ?? 'response');
  let xmlRootAttributes = $state(initialStructured.rootAttributes ?? []);
  let textContent = $state(initialStructured.textContent ?? '');

  // The template of a structured view, built from the state held here and never through the builders: they are
  // unmounted whenever the fieldset is folded or hidden (a proxy rule), or the other view of the format shows, and the
  // body must not depend on which one is mounted.
  function structuredTemplate(mode) {
    if (mode === 'json-paste' || mode === 'json-guided') {
      const object = fieldsToTemplate(mode === 'json-paste' ? jsonPasteFields : jsonFields);
      return jsonArrayRoot ? `[${object}]` : object;
    }
    return xmlFieldsToTemplate(mode === 'xml-paste' ? xmlPasteFields : xmlFields, xmlRootTag, xmlRootAttributes);
  }

  // pre_script, script and post_script run independently (same request context, no chaining between them); one
  // validation endpoint serves the three slots.
  let scriptEnabled = $state(!!init?.script);
  let scriptCode = $state(init?.script ?? '');
  let scriptValidation = $state({ status: '', message: '' });

  let preScriptEnabled = $state(!!init?.pre_script);
  let preScriptCode = $state(init?.pre_script ?? '');
  let preScriptValidation = $state({ status: '', message: '' });

  let postScriptEnabled = $state(!!init?.post_script);
  let postScriptCode = $state(init?.post_script ?? '');
  let postScriptValidation = $state({ status: '', message: '' });

  async function validateScriptCode(code) {
    if (!code.trim()) {
      return { status: 'error', message: t('The script is empty.') };
    }
    try {
      const result = await apiValidateScript(code);
      return result.valid ? { status: 'ok', message: t('Valid script.') } : { status: 'error', message: result.error };
    } catch (e) {
      return { status: 'error', message: e.message };
    }
  }

  async function handleValidateScript() {
    scriptValidation = { status: 'pending', message: t('Validating...') };
    scriptValidation = await validateScriptCode(scriptCode);
  }

  async function handleValidatePreScript() {
    preScriptValidation = { status: 'pending', message: t('Validating...') };
    preScriptValidation = await validateScriptCode(preScriptCode);
  }

  async function handleValidatePostScript() {
    postScriptValidation = { status: 'pending', message: t('Validating...') };
    postScriptValidation = await validateScriptCode(postScriptCode);
  }

  function buildFragmentsFromMode() {
    if (responseMode === 'empty') return [];
    if (STRUCTURED_MODES.includes(responseMode)) {
      return [{ type: 'Template', template: structuredTemplate(responseMode) }];
    }
    if (responseMode === 'text') {
      return [{ type: 'Literal', value: textContent }];
    }
    return fragments;
  }

  // Labels are getters, read when rendered, so that they follow a change of language.
  const fragmentTypes = [
    {
      value: 'Template',
      get label() {
        return t('Template (expressions)');
      },
    },
    {
      value: 'Literal',
      get label() {
        return t('Fixed text');
      },
    },
    {
      value: 'Uuid',
      get label() {
        return t('UUID v4');
      },
    },
    {
      value: 'PickFrom',
      get label() {
        return t('Random choice');
      },
    },
    {
      value: 'FakeData',
      get label() {
        return t('Fake data');
      },
    },
    {
      value: 'PathSegment',
      get label() {
        return t('URL segment (index)');
      },
    },
  ];

  const fakeKinds = [
    {
      value: 'FirstName',
      get label() {
        return t('First name');
      },
    },
    {
      value: 'LastName',
      get label() {
        return t('Last name');
      },
    },
    {
      value: 'Email',
      get label() {
        return t('Email address');
      },
    },
    {
      value: 'PhoneNumberFR',
      get label() {
        return t('Phone number (France)');
      },
    },
    {
      value: 'Integer',
      get label() {
        return t('Integer');
      },
    },
    {
      value: 'CompanyName',
      get label() {
        return t('Company name');
      },
    },
    {
      value: 'StreetName',
      get label() {
        return t('Street name');
      },
    },
    {
      value: 'CityFR',
      get label() {
        return t('City (France)');
      },
    },
    {
      value: 'PostcodeFR',
      get label() {
        return t('Postcode (France)');
      },
    },
    {
      value: 'Siren',
      get label() {
        return t('SIREN (French company number, 9 digits)');
      },
    },
    {
      value: 'Siret',
      get label() {
        return t('SIRET (French establishment number, 14 digits)');
      },
    },
    {
      value: 'FullAddressFR',
      get label() {
        return t('Full address (France)');
      },
    },
    {
      value: 'DatePast',
      get label() {
        return t('Past date');
      },
    },
    {
      value: 'DateFuture',
      get label() {
        return t('Future date');
      },
    },
    {
      value: 'TimestampMs',
      get label() {
        return t('Timestamp (ms)');
      },
    },
    {
      value: 'BoolRandom',
      get label() {
        return t('Random boolean');
      },
    },
    {
      value: 'LoremSentence',
      get label() {
        return t('Lorem ipsum sentence');
      },
    },
    {
      value: 'CountryFR',
      get label() {
        return t('French-speaking country');
      },
    },
    {
      value: 'IbanFR',
      get label() {
        return t('IBAN (France)');
      },
    },
  ];

  let pendingMode = $state(null);
  let modeKey = $state(0);
  let pendingConvMessage = $state('');

  function requestModeSwitch(newMode) {
    if (newMode === responseMode) return;
    const hasContent = currentModeHasContent();
    if (!hasContent) {
      applyModeSwitch(newMode);
      return;
    }
    const convResult = tryConvert(responseMode, newMode);
    if (convResult.ok) {
      applyModeSwitch(newMode, convResult);
      return;
    }
    pendingMode = newMode;
    pendingConvMessage = convResult.reason || '';
    modeKey++;
  }

  function confirmModeSwitch() {
    if (pendingMode) {
      applyModeSwitch(pendingMode);
      pendingMode = null;
      pendingConvMessage = '';
    }
  }

  function cancelModeSwitch() {
    pendingMode = null;
    pendingConvMessage = '';
    modeKey++;
  }

  function applyModeSwitch(newMode, convResult) {
    if (convResult?.jsonFields) jsonFields = convResult.jsonFields;
    if (convResult?.jsonPasteFields) jsonPasteFields = convResult.jsonPasteFields;
    if (convResult?.jsonArrayRoot !== undefined) jsonArrayRoot = convResult.jsonArrayRoot;
    if (convResult?.xmlFields) xmlFields = convResult.xmlFields;
    if (convResult?.xmlPasteFields) xmlPasteFields = convResult.xmlPasteFields;
    if (convResult?.xmlRootTag !== undefined) xmlRootTag = convResult.xmlRootTag;
    if (convResult?.xmlRootAttributes !== undefined) xmlRootAttributes = convResult.xmlRootAttributes;
    if (convResult?.textContent !== undefined) textContent = convResult.textContent;
    if (convResult?.fragments) fragments = convResult.fragments;
    responseMode = newMode;
    modeKey++;
  }

  // "Edit in detail": from the by-example view (json-paste, xml-paste) to the detailed one (json-guided, xml-guided),
  // with the same `fields` array, since both views share its shape. It bypasses the warning of
  // requestModeSwitch/tryConvert: it shows more controls over the same data rather than converting it.
  function revealDetailMode() {
    if (responseMode === 'json-paste') {
      jsonFields = jsonPasteFields;
      responseMode = 'json-guided';
    } else if (responseMode === 'xml-paste') {
      xmlFields = xmlPasteFields;
      responseMode = 'xml-guided';
    }
  }

  // The way back, mirror of revealDetailMode(): the same fields, copied without a warning. The by-example builder is
  // mounted again (Svelte changes {:else if} branch) and reads `startParsed={jsonPasteFields.length > 0}` then, so it
  // shows the fields rather than an empty paste area.
  function backToPasteMode() {
    if (responseMode === 'json-guided') {
      jsonPasteFields = jsonFields;
      responseMode = 'json-paste';
    } else if (responseMode === 'xml-guided') {
      xmlPasteFields = xmlFields;
      responseMode = 'xml-paste';
    }
  }

  // The four structured modes make two format buttons (JSON, XML); text, advanced and empty are one mode each.
  function formatOfMode(mode) {
    if (mode === 'json-paste' || mode === 'json-guided') return 'json';
    if (mode === 'xml-paste' || mode === 'xml-guided') return 'xml';
    return mode;
  }

  function selectFormat(fmt) {
    // Already this format (by example or in detail): a second click on its button must not reset the structure being
    // built.
    if (formatOfMode(responseMode) === fmt) return;
    const target = fmt === 'json' ? 'json-paste' : fmt === 'xml' ? 'xml-paste' : fmt;
    requestModeSwitch(target);
  }

  function currentModeHasContent() {
    if (responseMode === 'json-paste') return jsonPasteFields.length > 0;
    if (responseMode === 'json-guided') return jsonFields.length > 0;
    if (responseMode === 'xml-guided') return xmlFields.length > 0;
    if (responseMode === 'xml-paste') return xmlPasteFields.length > 0;
    if (responseMode === 'text') return textContent.trim().length > 0;
    if (responseMode === 'advanced')
      return fragments.some((f) => {
        if (f.type === 'Template') return f.template?.trim();
        if (f.type === 'Literal') return f.value?.trim();
        return true;
      });
    return false;
  }

  function getAdvancedTemplate() {
    return fragments
      .map((f) => {
        if (f.type === 'Literal') return f.value ?? '';
        if (f.type === 'Template') return f.template ?? '';
        return '';
      })
      .join('');
  }

  function tryConvert(from, to) {
    if (from === 'advanced' && to === 'text') {
      return { ok: true, textContent: getAdvancedTemplate() };
    }
    if (from === 'text' && to === 'advanced') {
      return { ok: true, fragments: [{ type: 'Literal', value: textContent }] };
    }
    if (from === 'advanced' && to === 'json-guided') {
      return tryAdvancedToJsonGuided();
    }
    if (from === 'advanced' && to === 'json-paste') {
      // The JSON button opens 'json-paste' (selectFormat): the same conversion as towards 'json-guided', stored in
      // jsonPasteFields.
      const r = tryAdvancedToJsonGuided();
      return r.ok ? { ok: true, jsonPasteFields: r.jsonFields, jsonArrayRoot: r.jsonArrayRoot } : r;
    }
    if (from === 'advanced' && to === 'xml-guided') {
      return tryAdvancedToXmlGuided();
    }
    if (from === 'advanced' && to === 'xml-paste') {
      const r = tryAdvancedToXmlGuided();
      return r.ok
        ? {
            ok: true,
            xmlPasteFields: r.xmlFields ?? [],
            xmlRootTag: r.xmlRootTag,
            xmlRootAttributes: r.xmlRootAttributes,
          }
        : r;
    }
    // Every structured view, by example or in detail, turns into its own template: nothing is lost.
    if (STRUCTURED_MODES.includes(from) && to === 'advanced') {
      return { ok: true, fragments: [{ type: 'Template', template: structuredTemplate(from) }] };
    }
    if (from === 'json-guided' && to === 'xml-guided') {
      return tryJsonFieldsToXmlFields(jsonFields);
    }
    if (from === 'json-guided' && to === 'xml-paste') {
      const r = tryJsonFieldsToXmlFields(jsonFields);
      return r.ok ? { ok: true, xmlPasteFields: r.xmlFields ?? [] } : r;
    }
    if (from === 'json-paste' && to === 'xml-paste') {
      // The same conversion, from jsonPasteFields (same shape as jsonFields).
      const r = tryJsonFieldsToXmlFields(jsonPasteFields);
      return r.ok ? { ok: true, xmlPasteFields: r.xmlFields ?? [] } : r;
    }
    if (from === 'xml-guided' && to === 'json-guided') {
      return {
        ok: false,
        reason: t('Converting XML to guided JSON is not supported. Go through the advanced template mode.'),
      };
    }
    if ((from === 'xml-guided' || from === 'xml-paste') && to === 'json-paste') {
      return {
        ok: false,
        reason: t('Converting XML to JSON is not supported. Go through the advanced template mode.'),
      };
    }
    return { ok: false };
  }

  function tryAdvancedToJsonGuided() {
    const tpl = getAdvancedTemplate();
    if (!tpl.trim()) return { ok: true, jsonFields: [], jsonArrayRoot: false };
    const jsonErr = validateTemplateAsJson(tpl);
    if (jsonErr) {
      return {
        ok: false,
        reason: t('Cannot convert: {0}. Check the braces: { and } are literal, {{ and }} enclose a variable.', jsonErr),
      };
    }
    try {
      const r = readJsonTemplate(tpl);
      return { ok: true, jsonFields: r.fields, jsonArrayRoot: r.arrayRoot };
    } catch (e) {
      return { ok: false, reason: t('Cannot convert: {0}', e.message) };
    }
  }

  // Succeeds for any well-formed XML template, like tryAdvancedToJsonGuided above: templateToXmlFields, which also
  // restores the view when a rule is reopened, reads the `{{expr | pipe}}` template back into fields, root tag and root
  // attributes included.
  function tryAdvancedToXmlGuided() {
    const tpl = getAdvancedTemplate();
    if (!tpl.trim()) return { ok: true, xmlFields: [] };
    const xmlErr = validateTemplateAsXml(tpl);
    if (xmlErr) {
      return { ok: false, reason: t('Cannot convert: {0}', xmlErr) };
    }
    try {
      const parsed = templateToXmlFields(tpl);
      return {
        ok: true,
        xmlFields: parsed.fields,
        xmlRootTag: parsed.rootTag,
        xmlRootAttributes: parsed.rootAttributes,
      };
    } catch (e) {
      return { ok: false, reason: t('Cannot convert: {0}', e.message) };
    }
  }

  // Takes either array of JSON fields, jsonFields (in detail) or jsonPasteFields (by example): they have the same shape.
  function tryJsonFieldsToXmlFields(sourceFields) {
    if (!sourceFields.length) return { ok: true, xmlFields: [] };
    try {
      const xmlF = sourceFields.filter((f) => f.key?.trim()).map((f) => jsonFieldToXmlNode(f));
      return { ok: true, xmlFields: xmlF };
    } catch {
      return {
        ok: false,
        reason: t('The JSON structure holds elements XML cannot express (arrays of scalar values).'),
      };
    }
  }

  function jsonFieldToXmlNode(f) {
    const ft = f.fieldType || 'value';
    if (ft === 'object') {
      return {
        tag: f.key,
        nodeType: 'parent',
        children: (f.children || []).filter((c) => c.key?.trim()).map((c) => jsonFieldToXmlNode(c)),
      };
    }
    if (ft === 'array-objects') {
      return {
        tag: f.key,
        nodeType: 'parent',
        children: (f.template || []).filter((c) => c.key?.trim()).map((c) => jsonFieldToXmlNode(c)),
      };
    }
    if (ft === 'array-values') {
      throw new Error('incompatible');
    }
    return { tag: f.key, nodeType: 'value', source: f.source || 'fixed', value: f.value || '' };
  }

  function addFragment() {
    fragments = [...fragments, { type: 'Literal', value: '' }];
  }
  function removeFragment(idx) {
    fragments = fragments.filter((_, i) => i !== idx);
  }
  function moveFragment(idx, dir) {
    const t = idx + dir;
    if (t < 0 || t >= fragments.length) return;
    const c = [...fragments];
    [c[idx], c[t]] = [c[t], c[idx]];
    fragments = c;
  }
  function updateFragmentType(idx, newType) {
    const c = [...fragments];
    if (newType === 'Literal') c[idx] = { type: 'Literal', value: '' };
    else if (newType === 'Uuid') c[idx] = { type: 'Uuid' };
    else if (newType === 'PickFrom') c[idx] = { type: 'PickFrom', values: [''] };
    else if (newType === 'FakeData') c[idx] = { type: 'FakeData', kind: { type: 'FirstName' } };
    else if (newType === 'PathSegment') c[idx] = { type: 'PathSegment', index: 0 };
    else if (newType === 'Template') c[idx] = { type: 'Template', template: '' };
    fragments = c;
  }
  function updateFakeKind(idx, kindType) {
    const c = [...fragments];
    c[idx] =
      kindType === 'Integer'
        ? { type: 'FakeData', kind: { type: 'Integer', min: 0, max: 100 } }
        : { type: 'FakeData', kind: { type: kindType } };
    fragments = c;
  }
  function addPickValue(idx) {
    const c = [...fragments];
    c[idx] = { ...c[idx], values: [...c[idx].values, ''] };
    fragments = c;
  }
  function removePickValue(fi, vi) {
    const c = [...fragments];
    c[fi] = { ...c[fi], values: c[fi].values.filter((_, i) => i !== vi) };
    fragments = c;
  }

  const commonHeaders = [
    'Content-Type',
    'Accept',
    'Authorization',
    'Cache-Control',
    'X-Request-Id',
    'X-Correlation-Id',
    'X-Forwarded-For',
    'Access-Control-Allow-Origin',
    'Access-Control-Allow-Methods',
  ];

  const commonContentTypes = [
    'application/json',
    'application/xml',
    'text/plain',
    'text/html',
    'application/x-www-form-urlencoded',
    'multipart/form-data',
    'application/octet-stream',
    'application/pdf',
  ];

  function addHeader() {
    respHeaders = [...respHeaders, { name: '', value: '' }];
  }
  function removeHeader(idx) {
    respHeaders = respHeaders.filter((_, i) => i !== idx);
  }

  export function validate() {
    if (responseMode === 'json-paste') {
      const err = validateTemplateAsJson(structuredTemplate(responseMode));
      if (err) return t('Invalid example JSON: {0}', err);
    }
    if (responseMode === 'json-guided') {
      const err = validateTemplateAsJson(structuredTemplate(responseMode));
      if (err) return t('Invalid guided JSON: {0}', err);
    }
    if (responseMode === 'xml-paste' || responseMode === 'xml-guided') {
      const err = validateTemplateAsXml(structuredTemplate(responseMode));
      if (err) return err;
    }
    if (responseMode === 'advanced') {
      const tpl = getAdvancedTemplate();
      const ct = respHeaders.find((h) => h.name?.toLowerCase() === 'content-type')?.value?.toLowerCase() || '';
      if (ct.includes('json') && tpl.trim()) {
        const err = validateTemplateAsJson(tpl);
        if (err) return t('JSON Content-Type but invalid template: {0}', err);
      }
      if (ct.includes('xml') && tpl.trim()) {
        const xmlErr = validateTemplateAsXml(tpl);
        if (xmlErr) return t('XML Content-Type but invalid template: {0}', xmlErr);
      }
    }
    return null;
  }

  export function getPayload() {
    const finalStatus = responseMode === 'empty' ? 204 : status;
    const finalHeaders = responseMode === 'empty' ? [] : respHeaders.filter((h) => h.name.trim());
    if (
      (responseMode === 'json-guided' || responseMode === 'json-paste') &&
      !finalHeaders.some((h) => h.name.toLowerCase() === 'content-type')
    ) {
      finalHeaders.push({ name: 'Content-Type', value: 'application/json' });
    }
    if (
      (responseMode === 'xml-guided' || responseMode === 'xml-paste') &&
      !finalHeaders.some((h) => h.name.toLowerCase() === 'content-type')
    ) {
      finalHeaders.push({ name: 'Content-Type', value: 'application/xml' });
    }
    const finalBody = buildFragmentsFromMode();
    return {
      response: {
        status: finalStatus,
        headers: finalHeaders,
        body: finalBody,
        chaos: chaosEnabled ? chaos : null,
      },
      pre_script: preScriptEnabled && preScriptCode.trim() ? preScriptCode.trim() : null,
      script: scriptEnabled && scriptCode.trim() ? scriptCode.trim() : null,
      post_script: postScriptEnabled && postScriptCode.trim() ? postScriptCode.trim() : null,
      // Records the view, to reopen it next time. The values of `responseMode` are the kebab-case variants of
      // ResponseEditorMode (src/models/mod.rs), so no mapping is needed.
      response_mode: responseMode,
    };
  }
</script>

{#snippet basicScriptHelp(varName)}
  <p class="field-hint">
    {t('Runs independently of the other script blocks (same request context, no chaining).')}
    <Sentence text={t('Result available as {0} or {1}.')} codes={[`{{${varName}}}`, `{{${varName}.field}}`]} />
    {t('Same Rhai syntax as the custom script below (see its examples).')}
  </p>
{/snippet}

{#snippet mainScriptHelp()}
  <p class="field-hint">
    <strong>{t('Available context:')}</strong>
    <Sentence
      text={t('{0} (text), {1}, {2}, {3} (key/value maps)')}
      codes={['request.body', 'request.headers', 'request.query', 'request.path']}
    />
  </p>
  <p class="field-hint">
    <strong>{t('Result:')}</strong>
    <Sentence
      text={t('The last expression is returned. A string → {0}. An object {1} → {2}')}
      codes={['{{script}}', '#{key: val}', '{{script.key}}']}
    />
  </p>
  <details class="script-examples">
    <summary class="field-hint">{t('Rhai examples and syntax')}</summary>
    <div class="script-examples-content">
      <p><strong>{t('Variables:')}</strong> <code>let x = 42;</code> <code>let s = "hello";</code></p>
      <p><strong>{t('Conditions:')}</strong> <code>if x &gt; 10 {'{'} "big" {'}'} else {'{'} "small" {'}'}</code></p>
      <p>
        <strong>{t('Strings:')}</strong> <code>s.to_upper()</code> <code>s.len()</code> <code>s.contains("el")</code>
        <code>s.replace("a", "b")</code>
      </p>
      <p>
        <strong>{t('Available functions and context data')}</strong>
        {t('(autocompletion in the editor: type the start of a name, or Ctrl+Space):')}
      </p>
      <ul class="script-fn-list">
        {#each RHAI_FUNCTIONS as fn}
          <li><code>{fn.signature}</code> — {fn.description}</li>
        {/each}
      </ul>
      <p>
        <strong>{t('Returned object:')}</strong> <code>#{'{'} key: "val", n: random_int(1,100) {'}'}</code>
        <Sentence text={t('available as {0}')} codes={['{{script.key}}']} />
      </p>
      <p>
        <strong>{t('4 in 5 ratio:')}</strong>
        <code
          >if random_int(1,5) &lt;= 4 {'{'} #{'{'} status: "ok" {'}'}
          {'}'} else {'{'} #{'{'} status: "ko" {'}'}
          {'}'}</code
        >
      </p>
      <p>
        <strong>{t('Same value for the same id:')}</strong>
        <code>seeded_pick(request.path.id, ["Acme Ltd", "Globex Corp"])</code>
      </p>
      <p class="field-hint">
        {t('Sandbox: no file or network access, 10,000 operations at most.')}
        <a href="https://rhai.rs/book/" target="_blank" rel="noopener">{t('Rhai documentation')}</a>
      </p>
    </div>
  </details>
{/snippet}

{#if visible}
  <fieldset class="section section-response">
    <legend>
      <button
        type="button"
        class="legend-toggle"
        onclick={() => (responseOpen = !responseOpen)}
        aria-expanded={responseOpen}
        data-testid="rule-form-response-toggle-button"
      >
        {responseOpen ? '▼' : '▶'}
        {t('Mocked response')}
      </button>
    </legend>

    {#if responseOpen}
      {#key modeKey}
        <!--
      One button per format. JSON and XML each have two views, by example and in detail: the "Edit in detail" button
      below switches between them (revealDetailMode), so the selector stays a single row.
    -->
        <div class="mode-selector" role="radiogroup" aria-label={t('Response format')}>
          {#each [['json', t('JSON')], ['xml', t('XML')], ['text', t('Text')], ['advanced', t('Advanced template')], ['empty', t('Empty (204)')]] as [val, label]}
            <button
              type="button"
              class="mode-btn"
              class:mode-active={formatOfMode(responseMode) === val}
              onclick={() => selectFormat(val)}
              role="radio"
              aria-checked={formatOfMode(responseMode) === val}
              data-testid="rule-form-mode-button-{val}">{label}</button
            >
          {/each}
        </div>
      {/key}

      {#if pendingMode}
        <div class="callout callout-warning" role="alert">
          <p>{pendingConvMessage || t('Switching to the "{0}" mode may lose data.', pendingMode)}</p>
          <div class="callout-actions">
            <button
              type="button"
              class="btn btn-sm btn-primary"
              onclick={confirmModeSwitch}
              data-testid="rule-form-mode-switch-confirm-button">{t('Switch anyway')}</button
            >
            <button
              type="button"
              class="btn btn-sm btn-secondary"
              onclick={cancelModeSwitch}
              data-testid="rule-form-mode-switch-cancel-button">{t('Cancel')}</button
            >
          </div>
        </div>
      {/if}

      {#if responseMode !== 'empty'}
        <div class="form-row">
          <div class="form-field" style="max-width:8rem">
            <label for="resp-status">{t('HTTP status')}</label>
            <input
              id="resp-status"
              type="number"
              bind:value={status}
              min="100"
              max="599"
              data-testid="rule-form-status-input"
            />
          </div>
        </div>

        <div class="sub-section">
          <strong>{t('Headers')}</strong>
          {#each respHeaders as hdr, idx}
            <div class="header-row">
              <input
                type="text"
                bind:value={hdr.name}
                placeholder="Content-Type"
                aria-label={t('Name of the header {0}', idx + 1)}
                list="dl-header-names"
                autocomplete="off"
                data-testid="rule-form-header-name-input-{idx}"
              />
              <input
                type="text"
                bind:value={hdr.value}
                placeholder="application/json"
                aria-label={t('Value of the header {0}', idx + 1)}
                list={hdr.name?.toLowerCase() === 'content-type' ? 'dl-content-types' : undefined}
                autocomplete="off"
                data-testid="rule-form-header-value-input-{idx}"
              />
              <button
                type="button"
                class="btn-icon btn-icon-s btn-delete"
                onclick={() => removeHeader(idx)}
                aria-label={t('Delete the header')}
                data-testid="rule-form-remove-header-button-{idx}">&#10005;</button
              >
            </div>
          {/each}
          <datalist id="dl-header-names">
            {#each commonHeaders as h}<option value={h}></option>{/each}
          </datalist>
          <datalist id="dl-content-types">
            {#each commonContentTypes as ct}<option value={ct}></option>{/each}
          </datalist>
          <button
            type="button"
            class="btn btn-sm btn-outline"
            onclick={addHeader}
            data-testid="rule-form-add-header-button">{t('+ Header')}</button
          >
          {#if responseMode === 'json-guided' || responseMode === 'json-paste'}
            <span class="field-hint">{t('Content-Type: application/json is added automatically.')}</span>
          {/if}
        </div>
      {/if}

      {#if responseMode === 'json-paste'}
        <div class="sub-section">
          <JsonPasteBuilder
            fields={jsonPasteFields}
            startParsed={jsonPasteFields.length > 0}
            arrayRoot={jsonArrayRoot}
            onUpdate={(f) => (jsonPasteFields = f)}
            onArrayRootChange={(v) => (jsonArrayRoot = v)}
          />
          <button
            type="button"
            class="btn btn-sm btn-outline open-detail-button"
            onclick={revealDetailMode}
            data-testid="rule-form-open-detail-button"
          >
            {t('Edit in detail (full structure) →')}
          </button>
        </div>
      {:else if responseMode === 'json-guided'}
        <div class="sub-section">
          <JsonResponseBuilder fields={jsonFields} arrayRoot={jsonArrayRoot} onUpdate={(f) => (jsonFields = f)} />
          <button
            type="button"
            class="btn btn-sm btn-outline back-to-paste-button"
            onclick={backToPasteMode}
            data-testid="rule-form-back-to-paste-button"
          >
            {t('← Back to the “by example” view')}
          </button>
        </div>
      {:else if responseMode === 'xml-paste'}
        <div class="sub-section">
          <XmlPasteBuilder
            fields={xmlPasteFields}
            rootTag={xmlRootTag}
            rootAttributes={xmlRootAttributes}
            startParsed={xmlPasteFields.length > 0}
            onUpdate={(f) => (xmlPasteFields = f)}
            onRootChange={(root) => {
              xmlRootTag = root.rootTag;
              xmlRootAttributes = root.rootAttributes;
            }}
          />
          <button
            type="button"
            class="btn btn-sm btn-outline open-detail-button"
            onclick={revealDetailMode}
            data-testid="rule-form-open-detail-button"
          >
            {t('Edit in detail (full structure) →')}
          </button>
        </div>
      {:else if responseMode === 'xml-guided'}
        <div class="sub-section">
          <XmlResponseBuilder
            fields={xmlFields}
            rootTag={xmlRootTag}
            rootAttributes={xmlRootAttributes}
            onUpdate={(f) => (xmlFields = f)}
            onRootTagChange={(tag) => (xmlRootTag = tag)}
          />
          <button
            type="button"
            class="btn btn-sm btn-outline back-to-paste-button"
            onclick={backToPasteMode}
            data-testid="rule-form-back-to-paste-button"
          >
            {t('← Back to the “by example” view')}
          </button>
        </div>
      {:else if responseMode === 'text'}
        <div class="sub-section">
          <strong>{t('Text content')}</strong>
          <textarea
            bind:value={textContent}
            rows="5"
            placeholder={t('Response content as plain text')}
            aria-label={t('Text content of the response')}
            class="text-area"
            data-testid="rule-form-text-content-textarea"></textarea>
        </div>
      {:else if responseMode === 'advanced'}
        <div class="sub-section">
          <strong>{t('Response body (fragments)')}</strong>
          <p class="section-help">{t('Build the response from blocks joined in order.')}</p>

          {#each fragments as frag, idx}
            <div class="fragment-card" data-testid="rule-form-fragment-card-{idx}">
              <div class="fragment-header">
                <span class="frag-index">{idx + 1}</span>
                <select
                  value={frag.type}
                  onchange={(e) => updateFragmentType(idx, e.target.value)}
                  aria-label={t('Type of the fragment {0}', idx + 1)}
                  data-testid="rule-form-fragment-type-select-{idx}"
                >
                  {#each fragmentTypes as ft}
                    <option value={ft.value}>{ft.label}</option>
                  {/each}
                </select>
                <div class="fragment-actions">
                  <button
                    type="button"
                    class="btn-icon btn-icon-s"
                    onclick={() => moveFragment(idx, -1)}
                    disabled={idx === 0}
                    aria-label={t('Move up')}
                    title={t('Move up')}
                    data-testid="rule-form-fragment-moveup-button-{idx}">&#9650;</button
                  >
                  <button
                    type="button"
                    class="btn-icon btn-icon-s"
                    onclick={() => moveFragment(idx, 1)}
                    disabled={idx === fragments.length - 1}
                    aria-label={t('Move down')}
                    title={t('Move down')}
                    data-testid="rule-form-fragment-movedown-button-{idx}">&#9660;</button
                  >
                  <button
                    type="button"
                    class="btn-icon btn-icon-s btn-delete"
                    onclick={() => removeFragment(idx)}
                    aria-label={t('Delete')}
                    title={t('Delete')}
                    data-testid="rule-form-fragment-delete-button-{idx}">&#10005;</button
                  >
                </div>
              </div>
              <div class="fragment-body">
                {#if frag.type === 'Literal'}
                  <textarea
                    bind:value={frag.value}
                    rows="2"
                    placeholder={t('e.g. {0}', '{"id":"')}
                    aria-label={t('Text content')}
                    data-testid="rule-form-fragment-literal-textarea-{idx}"></textarea>
                {:else if frag.type === 'Uuid'}
                  <p class="frag-info">{t('A UUID v4 generated for each request.')}</p>
                {:else if frag.type === 'PickFrom'}
                  {#each frag.values as val, vi}
                    <div class="pick-row">
                      <input
                        type="text"
                        bind:value={frag.values[vi]}
                        placeholder={t('Value {0}', vi + 1)}
                        aria-label={t('Value {0}', vi + 1)}
                        data-testid="rule-form-fragment-pick-input-{idx}-{vi}"
                      />
                      <button
                        type="button"
                        class="btn-icon btn-icon-s btn-delete"
                        onclick={() => removePickValue(idx, vi)}
                        aria-label={t('Delete')}
                        data-testid="rule-form-fragment-pick-remove-button-{idx}-{vi}">&#10005;</button
                      >
                    </div>
                  {/each}
                  <button
                    type="button"
                    class="btn btn-sm btn-outline"
                    onclick={() => addPickValue(idx)}
                    data-testid="rule-form-fragment-pick-add-button-{idx}">{t('+ Value')}</button
                  >
                {:else if frag.type === 'FakeData'}
                  <select
                    value={frag.kind?.type ?? 'FirstName'}
                    onchange={(e) => updateFakeKind(idx, e.target.value)}
                    aria-label={t('Kind of fake data')}
                    data-testid="rule-form-fragment-fake-select-{idx}"
                  >
                    {#each fakeKinds as fk}<option value={fk.value}>{fk.label}</option>{/each}
                  </select>
                {:else if frag.type === 'PathSegment'}
                  <label class="inline-label"
                    >{t('Position')}
                    <input
                      type="number"
                      bind:value={frag.index}
                      min="0"
                      style="width:5rem"
                      data-testid="rule-form-fragment-pathsegment-input-{idx}"
                    /></label
                  >
                {:else if frag.type === 'Template'}
                  <textarea
                    bind:value={frag.template}
                    rows="5"
                    class="template-textarea"
                    placeholder={t('e.g. {0}', '{"id":"{{path.id}}","code":"{{path.id | first(3)}}"}')}
                    aria-label={t('Template')}
                    data-testid="rule-form-fragment-template-textarea-{idx}"></textarea>
                  <div class="template-help">
                    <span class="field-hint"
                      ><strong>{t('Variables:')}</strong> <code>{`{{path.name}}`}</code>, <code>{`{{query.id}}`}</code>,
                      <code>{`{{uuid}}`}</code>, <code>{`{{now_ms}}`}</code>, <code>{`{{fake.CompanyName}}`}</code>,
                      <code>{`{{seq}}`}</code></span
                    >
                    <span class="field-hint"
                      ><strong>{t('Pipes:')}</strong> <code>| lower</code>, <code>| upper</code>,
                      <code>| capitalize</code>, <code>| first(N)</code>, <code>| last(N)</code>,
                      <code>| substr(start,len)</code>, <code>| replace("a","b")</code>, <code>| prepend("x")</code>,
                      <code>| append("x")</code>, <code>| default("val")</code>, <code>| length</code>,
                      <code>| trim</code>. {t('Single braces { } are literal text.')}</span
                    >
                  </div>
                {/if}
              </div>
            </div>
          {/each}
          <button
            type="button"
            class="btn btn-sm btn-outline"
            onclick={addFragment}
            data-testid="rule-form-fragment-add-button">{t('+ Add a fragment')}</button
          >
        </div>
      {:else if responseMode === 'empty'}
        <p class="section-help" style="margin-top: var(--space-2)">
          {t('The response will be 204 No Content, without a body.')}
        </p>
      {/if}

      <RuleScriptSlot
        id="rule-script"
        toggleLabel={t('Custom script')}
        enabled={scriptEnabled}
        code={scriptCode}
        onToggle={(v) => (scriptEnabled = v)}
        onCodeInput={(v) => (scriptCode = v)}
        validation={scriptValidation}
        onValidate={handleValidateScript}
        rows={8}
        placeholder={t(
          '// Rhai examples:\n// Return a simple value:\nlet id = request.path.id;\n`user_${id}`\n\n// Return an object (available as {{script.field}}):\n#{ name: "Alice", age: "30" }',
        )}
      >
        {#snippet help()}{@render mainScriptHelp()}{/snippet}
      </RuleScriptSlot>

      <div class="sub-section advanced-options-section">
        <button
          type="button"
          class="legend-toggle"
          onclick={() => (advancedOpen = !advancedOpen)}
          aria-expanded={advancedOpen}
          aria-controls="rule-form-advanced-options-panel"
          data-testid="rule-form-advanced-options-toggle-button"
        >
          {advancedOpen ? '▼' : '▶'}
          {t('Advanced options (pre-script / post-script)')}
        </button>
        <div
          id="rule-form-advanced-options-panel"
          class="advanced-options-panel"
          hidden={!advancedOpen}
          data-testid="rule-form-advanced-options-panel"
        >
          <RuleScriptSlot
            id="rule-pre-script"
            toggleLabel={t('Pre-script (preparation)')}
            enabled={preScriptEnabled}
            code={preScriptCode}
            onToggle={(v) => (preScriptEnabled = v)}
            onCodeInput={(v) => (preScriptCode = v)}
            validation={preScriptValidation}
            onValidate={handleValidatePreScript}
            rows={5}
          >
            {#snippet help()}{@render basicScriptHelp('pre_script')}{/snippet}
          </RuleScriptSlot>

          <RuleScriptSlot
            id="rule-post-script"
            toggleLabel={t('Post-script (finalization)')}
            enabled={postScriptEnabled}
            code={postScriptCode}
            onToggle={(v) => (postScriptEnabled = v)}
            onCodeInput={(v) => (postScriptCode = v)}
            validation={postScriptValidation}
            onValidate={handleValidatePostScript}
            rows={5}
          >
            {#snippet help()}{@render basicScriptHelp('post_script')}{/snippet}
          </RuleScriptSlot>
        </div>
      </div>

      <div class="sub-section chaos-section">
        <ToggleSwitch
          label={t('Chaos mode')}
          name="chaos"
          checked={chaosEnabled}
          onchange={(v) => (chaosEnabled = v)}
        />
        {#if chaosEnabled}
          <div class="chaos-fields">
            <label
              >{t('Fixed latency (ms)')}
              <input
                type="number"
                bind:value={chaos.delay_ms}
                min="0"
                max="30000"
                data-testid="rule-form-chaos-delay-input"
              /></label
            >
            <label
              >{t('Minimum latency (ms)')}
              <input
                type="number"
                bind:value={chaos.delay_min_ms}
                min="0"
                max="30000"
                data-testid="rule-form-chaos-delay-min-input"
              /></label
            >
            <label
              >{t('Maximum latency (ms)')}
              <input
                type="number"
                bind:value={chaos.delay_max_ms}
                min="0"
                max="30000"
                data-testid="rule-form-chaos-delay-max-input"
              /></label
            >
            <label
              >{t('Error rate (0-1)')}
              <input
                type="number"
                bind:value={chaos.error_rate}
                min="0"
                max="1"
                step="0.05"
                data-testid="rule-form-chaos-error-rate-input"
              /></label
            >
            <label
              >{t('Error status')}
              <input
                type="number"
                bind:value={chaos.error_status}
                min="400"
                max="599"
                data-testid="rule-form-chaos-error-status-input"
              /></label
            >
          </div>
          <span class="field-hint"
            >{t('When min and max are set, the latency is random in that range (the fixed latency is ignored).')}</span
          >
        {/if}
      </div>
    {/if}
  </fieldset>
{/if}

<style>
  .legend-toggle {
    background: none;
    border: none;
    font: inherit;
    font-weight: var(--weight-strong);
    font-size: var(--text-m);
    cursor: pointer;
    padding: 0;
    color: var(--color-text);
  }

  .mode-selector {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
    flex-wrap: wrap;
  }
  .mode-btn {
    font-size: var(--text-m);
    font-weight: var(--weight-medium);
    cursor: pointer;
    padding: var(--space-1-5) var(--space-3);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    background: var(--color-bg);
    color: var(--color-text);
    font-family: inherit;
  }
  .mode-btn:hover {
    border-color: var(--color-primary);
  }
  .mode-btn.mode-active {
    border-color: var(--color-primary);
    background: var(--color-selected);
    font-weight: var(--weight-strong);
  }

  .open-detail-button,
  .back-to-paste-button {
    margin-top: var(--space-2);
  }

  .advanced-options-section {
    border-top-color: var(--color-border);
  }
  .advanced-options-panel {
    margin-top: var(--space-2);
  }

  .header-row {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    margin-bottom: var(--space-1-5);
  }
  .header-row input {
    flex: 1;
    min-width: 0;
    padding: var(--space-1-5) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-m);
  }

  .fragment-card {
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-3);
    margin-bottom: var(--space-2);
    background: var(--color-bg);
  }
  .fragment-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .fragment-header select {
    flex: 1;
    min-width: 0;
    padding: var(--space-1-5) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-m);
  }
  .fragment-actions {
    display: flex;
    gap: var(--space-1);
    flex-shrink: 0;
  }
  .fragment-body {
    margin-top: var(--space-2);
  }
  .fragment-body textarea {
    width: 100%;
    padding: var(--space-1-5) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-m);
    font-family: var(--font-code);
    resize: vertical;
  }
  .fragment-body select {
    width: 100%;
    padding: var(--space-1-5) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-m);
    margin-bottom: var(--space-1-5);
  }
  .frag-index {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.5rem;
    height: 1.5rem;
    border-radius: var(--radius-round);
    background: var(--color-primary);
    color: var(--color-on-primary);
    font-size: var(--text-xs);
    font-weight: var(--weight-heavy);
    flex-shrink: 0;
  }
  .frag-info {
    font-size: var(--text-s);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
  }

  .text-area {
    width: 100%;
    padding: var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-m);
    font-family: inherit;
    resize: vertical;
  }
  .template-textarea {
    min-height: 5rem;
  }
  .template-help {
    margin-top: var(--space-1-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-0-5);
  }
  .template-help code {
    background: var(--color-bg);
    padding: var(--space-0-5) var(--space-1);
    border-radius: var(--radius-s);
    font-size: var(--text-s);
  }

  .pick-row {
    display: flex;
    gap: var(--space-1-5);
    align-items: center;
    margin-bottom: var(--space-1);
  }
  .pick-row input {
    flex: 1;
    min-width: 0;
    padding: var(--space-1-5) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-m);
  }

  .inline-label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-m);
    font-weight: var(--weight-medium);
    margin-bottom: var(--space-1-5);
  }
  .inline-label input {
    padding: var(--space-1-5) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-m);
  }

  .chaos-section {
    border-top-color: var(--color-warning);
  }
  .chaos-fields {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    margin-top: var(--space-2);
  }
  .chaos-fields label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-m);
    min-width: 8rem;
  }
  .chaos-fields input {
    padding: var(--space-1-5) var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    border-radius: var(--radius-m);
    font-size: var(--text-m);
  }
</style>
