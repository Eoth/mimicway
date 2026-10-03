import { t } from './i18n.svelte.js';
/**
 * Template utilities for Mimicway's template format.
 *
 * Template format (consumed by Rust backend):
 *   { and } = literal braces (normal JSON/XML)
 *   {{expr}} = template variable (evaluated at runtime)
 *   {{expr | pipe}} = variable with transformation
 *
 * This module is the SINGLE source of truth for parsing and validating
 * this format on the frontend side.
 */

// ── Serialization: Fields → Template string ──────────────────────────

export function fieldsToTemplate(fields) {
  if (fields.length === 0) return '{}';
  const parts = fields.filter(f => f.key?.trim()).map(f => fieldToTpl(f));
  return `{${parts.join(',')}}`;
}

function fieldToTpl(field) {
  const k = field.key?.trim();
  if (!k) return '';
  const ft = field.fieldType || 'value';

  if (ft === 'value') {
    const expr = buildExpr(field);
    return field.asNumber ? `"${k}":${expr}` : `"${k}":"${expr}"`;
  }
  if (ft === 'object') {
    const inner = (field.children || []).filter(c => c.key?.trim()).map(c => fieldToTpl(c)).join(',');
    return `"${k}":{${inner}}`;
  }
  if (ft === 'array-values') {
    const items = (field.items || []).map(item => {
      const expr = buildExpr(item);
      return item.asNumber ? expr : `"${expr}"`;
    }).join(',');
    return `"${k}":[${items}]`;
  }
  if (ft === 'array-objects') {
    const inner = (field.template || []).filter(c => c.key?.trim()).map(c => fieldToTpl(c)).join(',');
    return `"${k}":[{${inner}}]`;
  }
  return '';
}

export function buildExpr(f) {
  if (f.source === 'fixed') return f.value ?? '';
  let varPart;
  switch (f.source) {
    case 'path': varPart = `path.${f.value}`; break;
    case 'query': varPart = `query.${f.value}`; break;
    case 'header': varPart = `header.${f.value}`; break;
    case 'body': varPart = `body.${f.value}`; break;
    case 'xpath': varPart = `xpath.${f.value}`; break;
    case 'fake': varPart = `fake.${f.value}`; break;
    case 'script': varPart = f.value ? `script.${f.value}` : 'script'; break;
    case 'uuid': varPart = 'uuid'; break;
    case 'now_ms': varPart = 'now_ms'; break;
    case 'now_iso': varPart = 'now_iso'; break;
    case 'seq': varPart = 'seq'; break;
    default: return f.value ?? '';
  }
  const pipe = f.pipe?.trim();
  return pipe ? `{{${varPart} | ${pipe}}}` : `{{${varPart}}}`;
}

// ── Validation: Template string → test JSON ──────────────────────────

export function templateToTestJson(tpl) {
  let out = '';
  let i = 0;
  let inString = false;
  while (i < tpl.length) {
    if (tpl[i] === '\\' && inString) {
      out += tpl[i] + tpl[i + 1]; i += 2; continue;
    }
    if (tpl[i] === '{' && i + 1 < tpl.length && tpl[i + 1] === '{') {
      const end = findDoubleClose(tpl, i + 2);
      if (end !== -1) {
        out += inString ? '__var__' : '"__var__"';
        i = end + 2;
        continue;
      }
    }
    if (tpl[i] === '"') inString = !inString;
    out += tpl[i]; i++;
  }
  return out;
}

function findDoubleClose(str, start) {
  let inQuotes = false;
  let parenDepth = 0;
  for (let i = start; i < str.length; i++) {
    if (str[i] === '"' && parenDepth === 0) inQuotes = !inQuotes;
    if (str[i] === '(' && !inQuotes) parenDepth++;
    if (str[i] === ')' && !inQuotes) parenDepth = Math.max(0, parenDepth - 1);
    if (!inQuotes && parenDepth === 0 && str[i] === '}' && i + 1 < str.length && str[i + 1] === '}') return i;
  }
  return -1;
}

export function validateTemplateAsJson(tpl) {
  if (!tpl.trim()) return null;
  const testStr = templateToTestJson(tpl);
  try {
    JSON.parse(testStr);
    return null;
  } catch (e) {
    return t("Invalid JSON: {0}", e.message);
  }
}

export function validateTemplateAsXml(tpl) {
  if (!tpl.trim()) return null;
  const testXml = stripTemplateVars(tpl);
  try {
    const parser = new DOMParser();
    const doc = parser.parseFromString(testXml, 'application/xml');
    if (doc.querySelector('parsererror')) {
      return t("Invalid XML: check the tags (empty names, wrong nesting).");
    }
    return null;
  } catch {
    return t("Malformed XML.");
  }
}

function stripTemplateVars(tpl) {
  let out = '';
  let i = 0;
  while (i < tpl.length) {
    if (tpl[i] === '{' && i + 1 < tpl.length && tpl[i + 1] === '{') {
      const end = findDoubleClose(tpl, i + 2);
      if (end !== -1) { out += 'x'; i = end + 2; continue; }
    }
    out += tpl[i]; i++;
  }
  return out;
}

// ── Preview: Template string → human-readable ────────────────────────

export function templateToPreview(tpl) {
  let out = '';
  let i = 0;
  while (i < tpl.length) {
    if (tpl[i] === '{' && i + 1 < tpl.length && tpl[i + 1] === '{') {
      const end = findDoubleClose(tpl, i + 2);
      if (end !== -1) {
        out += `«${tpl.slice(i, end + 2)}»`;
        i = end + 2;
        continue;
      }
    }
    out += tpl[i]; i++;
  }
  return out;
}

// ── Deserialization: Template string → Fields ────────────────────────

export function templateToFields(tpl) {
  const trimmed = tpl.trim();
  if (!trimmed || trimmed === '{}') return [];
  const testStr = templateToTestJson(trimmed);
  const parsed = JSON.parse(testStr);
  if (typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new TypeError(t("The JSON must be an object to be shown in the guided view."));
  }
  return parseTplObject(trimmed);
}

function parseTplObject(tpl) {
  const entries = extractTplEntries(tpl);
  return entries.map(([key, rawValue]) => {
    const trimmed = rawValue.trim();
    if (trimmed.startsWith('{') && !isVarOpen(trimmed, 0)) {
      try {
        return { key, fieldType: 'object', children: parseTplObject(trimmed) };
      } catch { /* fall through */ }
    }
    if (trimmed.startsWith('[')) {
      return parseTplArray(key, trimmed);
    }
    return parseTplValue(key, trimmed);
  });
}

function isVarOpen(str, i) {
  return str[i] === '{' && i + 1 < str.length && str[i + 1] === '{';
}

function parseTplValue(key, raw) {
  const trimmed = raw.trim();
  const isQuoted = trimmed.startsWith('"') && trimmed.endsWith('"');
  const inner = isQuoted ? trimmed.slice(1, -1) : trimmed;

  const varMatch = inner.match(/^\{\{([^}].*?)\}\}$/);
  if (varMatch) {
    const expr = varMatch[1];
    const pipeIdx = findPipeSeparator(expr);
    let varName, pipes;
    if (pipeIdx >= 0) {
      varName = expr.slice(0, pipeIdx).trim();
      pipes = expr.slice(pipeIdx + 1).trim();
    } else {
      varName = expr.trim();
      pipes = '';
    }
    const { source, value } = varNameToSource(varName);
    return { key, fieldType: 'value', source, value, pipe: pipes, asNumber: !isQuoted };
  }
  return { key, fieldType: 'value', source: 'fixed', value: inner, pipe: '', asNumber: !isQuoted };
}

export function findPipeSeparator(expr) {
  let depth = 0;
  for (let i = 0; i < expr.length; i++) {
    if (expr[i] === '(') depth++;
    if (expr[i] === ')') depth--;
    if (expr[i] === '|' && depth === 0) return i;
  }
  return -1;
}

function parseTplArray(key, raw) {
  const inner = raw.trim().slice(1, -1).trim();
  if (!inner) return { key, fieldType: 'array-values', items: [] };
  if (inner.startsWith('{') && !isVarOpen(inner, 0)) {
    try {
      const template = parseTplObject(inner);
      return { key, fieldType: 'array-objects', template };
    } catch { /* fall through */ }
  }
  const items = splitTplArray(inner).map(item => {
    const f = parseTplValue('', item.trim());
    return { source: f.source, value: f.value, pipe: f.pipe || '', asNumber: f.asNumber };
  });
  return { key, fieldType: 'array-values', items };
}

export function varNameToSource(varName) {
  if (varName.startsWith('path.')) return { source: 'path', value: varName.slice(5) };
  if (varName.startsWith('query.')) return { source: 'query', value: varName.slice(6) };
  if (varName.startsWith('header.')) return { source: 'header', value: varName.slice(7) };
  if (varName.startsWith('body.')) return { source: 'body', value: varName.slice(5) };
  if (varName.startsWith('xpath.')) return { source: 'xpath', value: varName.slice(6) };
  if (varName.startsWith('fake.')) return { source: 'fake', value: varName.slice(5) };
  if (varName === 'script') return { source: 'script', value: '' };
  if (varName.startsWith('script.')) return { source: 'script', value: varName.slice(7) };
  if (varName === 'uuid') return { source: 'uuid', value: '' };
  if (varName === 'now_ms') return { source: 'now_ms', value: '' };
  if (varName === 'now_iso') return { source: 'now_iso', value: '' };
  if (varName === 'seq') return { source: 'seq', value: '' };
  return { source: 'fixed', value: varName };
}

// ── Example JSON: raw value → Fields (builder by example) ────────────
// Unlike templateToFields, which parses an existing {{...}} template, this starts from a literal JSON example, without
// {{}}, as the user pasted it: each value becomes a fixed field (fieldType 'value', source 'fixed') holding the pasted
// value, which the user can then bind to another source (path, query, fake data...) in the builder.
//
// A field holds the raw text of the template, as templateToFields gives it: a key or a string is the content of a JSON
// string, escapes included (JSON.parse decoded them), and any other value its JSON literal (null stays null). Keys go
// through the builder's rows like typed ones: trimmed, and a blank key is a row not filled in yet, left out.

export function exampleJsonToFields(value) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError(t("The example must be a JSON object at its root."));
  }
  return objectToFields(value);
}

function objectToFields(obj) {
  return Object.entries(obj).map(([name, value]) => {
    const key = rawJsonString(name);
    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
      return { key, fieldType: 'object', children: objectToFields(value) };
    }
    if (Array.isArray(value)) {
      if (value.length > 0 && typeof value[0] === 'object' && value[0] !== null) {
        return { key, fieldType: 'array-objects', template: objectToFields(value[0]) };
      }
      return { key, fieldType: 'array-values', items: value.map(v => ({ source: 'fixed', ...rawJsonValue(v), pipe: '' })) };
    }
    return { key, fieldType: 'value', source: 'fixed', ...rawJsonValue(value), pipe: '' };
  });
}

function rawJsonString(text) {
  return JSON.stringify(text).slice(1, -1);
}

function rawJsonValue(value) {
  return typeof value === 'string'
    ? { value: rawJsonString(value), asNumber: false }
    : { value: JSON.stringify(value), asNumber: true };
}

// ── Low-level JSON-aware parser ─────────────────────────────────────

function extractTplEntries(objStr) {
  const trimmed = objStr.trim();
  let inner = trimmed;
  if (trimmed.startsWith('{') && !isVarOpen(trimmed, 0)) {
    inner = trimmed.slice(1, -1);
  }

  const entries = [];
  let i = 0;
  while (i < inner.length) {
    while (i < inner.length && /[\s,]/.test(inner[i])) i++;
    if (i >= inner.length || inner[i] !== '"') break;
    // A key may hold escaped quotes (\"): read as a string token, not up to the next quote.
    const [quotedKey, keyLength] = readTplToken(inner, i);
    if (keyLength < 2 || !quotedKey.endsWith('"')) break;
    const key = quotedKey.slice(1, -1);
    i += keyLength;
    while (i < inner.length && /[\s:]/.test(inner[i])) i++;
    const [value, consumed] = readTplToken(inner, i);
    entries.push([key, value]);
    i += consumed;
  }
  return entries;
}

function readTplToken(str, start) {
  let i = start;
  if (i >= str.length) return ['', 0];

  if (str[i] === '"') {
    let j = i + 1;
    while (j < str.length) {
      if (str[j] === '\\') { j += 2; continue; }
      if (str[j] === '"') return [str.slice(i, j + 1), j + 1 - i];
      j++;
    }
    return [str.slice(i), str.length - i];
  }

  if (isVarOpen(str, i)) {
    const end = findDoubleClose(str, i + 2);
    if (end !== -1) return [str.slice(i, end + 2), end + 2 - i];
  }

  if (str[i] === '{') {
    let depth = 0, j = i, inStr = false;
    while (j < str.length) {
      if (str[j] === '\\' && inStr) { j += 2; continue; }
      if (str[j] === '"') { inStr = !inStr; j++; continue; }
      if (!inStr) {
        if (str[j] === '{' && j + 1 < str.length && str[j + 1] === '{') {
          const end = findDoubleClose(str, j + 2);
          if (end !== -1) { j = end + 2; continue; }
        }
        if (str[j] === '{') { depth++; j++; continue; }
        if (str[j] === '}') {
          depth--;
          if (depth === 0) return [str.slice(i, j + 1), j + 1 - i];
          j++; continue;
        }
      }
      j++;
    }
    return [str.slice(i), str.length - i];
  }

  if (str[i] === '[') {
    let depth = 0, j = i, inStr = false;
    while (j < str.length) {
      if (str[j] === '\\' && inStr) { j += 2; continue; }
      if (str[j] === '"') { inStr = !inStr; j++; continue; }
      if (!inStr) {
        if (str[j] === '[') { depth++; j++; continue; }
        if (str[j] === ']') {
          depth--;
          if (depth === 0) return [str.slice(i, j + 1), j + 1 - i];
          j++; continue;
        }
      }
      j++;
    }
    return [str.slice(i), str.length - i];
  }

  let j = i;
  while (j < str.length && str[j] !== ',' && str[j] !== '}' && str[j] !== ']') j++;
  return [str.slice(i, j).trim(), j - i];
}

function splitTplArray(inner) {
  const items = [];
  let i = 0, start = 0;
  while (i < inner.length) {
    if (inner[i] === '"') { const [, c] = readTplToken(inner, i); i += c; continue; }
    if (isVarOpen(inner, i)) { const end = findDoubleClose(inner, i + 2); if (end !== -1) { i = end + 2; continue; } }
    if (inner[i] === '{') { const [, c] = readTplToken(inner, i); i += c; continue; }
    if (inner[i] === '[') { const [, c] = readTplToken(inner, i); i += c; continue; }
    if (inner[i] === ',') { items.push(inner.slice(start, i)); start = i + 1; }
    i++;
  }
  if (start < inner.length) items.push(inner.slice(start));
  return items;
}

// ── XML: Fields → Template string ────────────────────────────────────
//
// `attributes` (optional): a list of {name, source, value, pipe} on a value or parent node, and on the root through
// `rootAttributes`; absent or empty, it adds nothing to the text. Attribute values are not escaped, quotes included,
// like the text of elements: the server inserts template values as they are (resolve_variable in
// src/engine/template.rs), so the template stays raw text from end to end.

export function xmlFieldsToTemplate(fields, rootTag = 'response', rootAttributes = []) {
  const attrs = xmlAttrsToTpl(rootAttributes);
  const inner = fields.filter(f => f.tag?.trim()).map(f => xmlNodeToTpl(f)).join('');
  return `<${rootTag}${attrs}>${inner}</${rootTag}>`;
}

function xmlAttrsToTpl(attributes) {
  return (attributes || [])
    .filter(a => a.name?.trim())
    .map(a => ` ${a.name.trim()}="${buildExpr(a)}"`)
    .join('');
}

function xmlNodeToTpl(field) {
  const t = field.tag?.trim();
  if (!t) return '';
  const attrs = xmlAttrsToTpl(field.attributes);
  if ((field.nodeType || 'value') === 'parent') {
    const inner = (field.children || []).filter(c => c.tag?.trim()).map(c => xmlNodeToTpl(c)).join('');
    return `<${t}${attrs}>${inner}</${t}>`;
  }
  return `<${t}${attrs}>${buildExpr(field)}</${t}>`;
}

// ── Example XML: raw text → Fields (builder by example, XML) ─────────
// The XML counterpart of exampleJsonToFields: starts from a literal XML example, without {{}}, as the user pasted it
// (typically a real SOAP response), and returns { rootTag, rootAttributes, fields }, where `fields` are the direct
// child elements of the root, in the shape XmlResponseBuilder.svelte uses (tag/nodeType/source/value/children), plus
// `attributes`.
//
// Deliberate limits:
//  - Namespace prefixes ("soap:Envelope") and xmlns/xmlns:* declarations stay literal text in tag and attribute names,
//    as DOMParser gives them (.tagName, .name); no prefix is resolved to its URI and no namespace is validated. That
//    is enough to rebuild a template faithful to the pasted XML, without ever failing on it.
//  - Mixed content (text and child elements in one node): the child elements win (a 'parent' node) and the node's
//    own text is dropped. API and SOAP responses rarely mix them: a leaf holds text, a parent holds elements.
//  - A root without any child element (text only) is refused with a message that says so.

// DOMParser gives text and attribute values decoded (&amp; read as &): a fixed value is written back into the template
// as it is, so it is kept as raw XML, its markup characters as entities.
function rawXmlText(text) {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function rawXmlAttribute(text) {
  return rawXmlText(text).replace(/"/g, '&quot;');
}

export function exampleXmlToFields(xmlString) {
  const text = xmlString.trim();
  if (!text) {
    throw new TypeError(t("Paste valid XML."));
  }
  const parser = new DOMParser();
  const doc = parser.parseFromString(text, 'application/xml');
  if (doc.querySelector('parsererror')) {
    throw new TypeError(t("Invalid XML: check the tags (empty names, wrong nesting)."));
  }
  const root = doc.documentElement;
  if (!root) {
    throw new TypeError(t("Invalid XML: no root element found."));
  }
  const childElements = Array.from(root.children || []);
  if (childElements.length === 0) {
    throw new TypeError(t("The XML root holds no nested element. Paste XML with at least one child element."));
  }
  return {
    rootTag: root.tagName,
    rootAttributes: xmlAttributesToFields(root),
    fields: childElements.map(xmlElementToField),
  };
}

// ── XML template string → Fields (back to the structured view) ──────
// The XML counterpart of templateToFields() (JSON). Unlike exampleXmlToFields, which starts from literal XML, this
// starts from a template that xmlFieldsToTemplate() built ({{expr | pipe}} in place) and rebuilds the Fields
// (tag/nodeType/source/value/pipe/children/attributes) that both XML builders use, guided (XmlResponseBuilder.svelte)
// and by example (XmlPasteBuilder.svelte). DOMParser reads it as exampleXmlToFields does ({, }, |, ( and ) are valid
// literal XML text, nothing to escape); only the leaves differ: a leaf that is a {{expr | pipe}} expression keeps its
// source and pipe instead of becoming a fixed value.
export function templateToXmlFields(tpl) {
  const text = tpl.trim();
  if (!text) {
    throw new TypeError(t("Empty XML template."));
  }
  const parser = new DOMParser();
  const doc = parser.parseFromString(text, 'application/xml');
  if (doc.querySelector('parsererror')) {
    throw new TypeError(t("Invalid XML template: it cannot be parsed back into the structured view."));
  }
  const root = doc.documentElement;
  if (!root) {
    throw new TypeError(t("Invalid XML template: no root element found."));
  }
  return {
    rootTag: root.tagName,
    rootAttributes: xmlAttributesToTplFields(root),
    fields: Array.from(root.children || []).map(xmlElementToTplField),
  };
}

function xmlAttributesToTplFields(el) {
  return Array.from(el.attributes || []).map(attr => ({
    name: attr.name, ...parseXmlLeafExpr(attr.value, rawXmlAttribute),
  }));
}

function xmlElementToTplField(el) {
  const tag = el.tagName;
  const attributes = xmlAttributesToTplFields(el);
  const childElements = Array.from(el.children || []);
  if (childElements.length > 0) {
    return { tag, nodeType: 'parent', attributes, children: childElements.map(xmlElementToTplField) };
  }
  return { tag, nodeType: 'value', attributes, ...parseXmlLeafExpr(el.textContent ?? '', rawXmlText) };
}

// Reads an XML leaf (element text or attribute value): when it is exactly one {{expr | pipe}} expression, splits it
// as parseTplValue() does for JSON (varNameToSource, findPipeSeparator); otherwise a fixed literal value. No asNumber
// here, unlike JSON: XML text has no quoted and unquoted values. `toRaw` turns a fixed value back into raw XML.
function parseXmlLeafExpr(raw, toRaw) {
  const trimmed = (raw ?? '').trim();
  const varMatch = trimmed.match(/^\{\{([^}].*?)\}\}$/);
  if (varMatch) {
    const expr = varMatch[1];
    const pipeIdx = findPipeSeparator(expr);
    let varName, pipe;
    if (pipeIdx >= 0) {
      varName = expr.slice(0, pipeIdx).trim();
      pipe = expr.slice(pipeIdx + 1).trim();
    } else {
      varName = expr.trim();
      pipe = '';
    }
    const { source, value } = varNameToSource(varName);
    return { source, value, pipe };
  }
  return { source: 'fixed', value: toRaw(trimmed), pipe: '' };
}

function xmlAttributesToFields(el) {
  return Array.from(el.attributes || []).map(attr => ({
    name: attr.name, source: 'fixed', value: rawXmlAttribute(attr.value), pipe: '',
  }));
}

function xmlElementToField(el) {
  const tag = el.tagName;
  const attributes = xmlAttributesToFields(el);
  const childElements = Array.from(el.children || []);
  if (childElements.length > 0) {
    return { tag, nodeType: 'parent', attributes, children: childElements.map(xmlElementToField) };
  }
  return { tag, nodeType: 'value', source: 'fixed', value: rawXmlText(el.textContent ?? ''), pipe: '', attributes };
}
