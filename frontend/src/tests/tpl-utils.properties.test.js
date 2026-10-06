// Properties of tpl-utils.js on generated templates and examples (fast-check). A template is raw text from end to end
// (the server inserts values as they are), so a field holds the raw text of its value: the content of a JSON string
// with its escapes, XML text with its entities. Every way into the builders must keep to that, or a template goes
// through the structured view and comes back different, or invalid.
import { describe, it, expect } from 'vitest';
import fc from 'fast-check';
import {
  exampleJsonToFields,
  exampleXmlToFields,
  fieldsToTemplate,
  templateToFields,
  templateToTestJson,
  templateToXmlFields,
  validateTemplateAsJson,
  xmlFieldsToTemplate,
} from '../lib/tpl-utils.js';

// A {{ in a fixed value cannot be told apart from an expression: the format has no escape for it.
const noExpression = (s) => !s.includes('{{');
const ident = fc.stringMatching(/^[A-Za-z_][A-Za-z0-9_.]{0,10}$/);
const pipe = fc.oneof(
  fc.constant(''),
  fc.stringMatching(/^[a-z_]{1,8}$/),
  fc.tuple(fc.stringMatching(/^[a-z_]{1,8}$/), fc.stringMatching(/^[a-z0-9_]{0,6}$/)).map(([f, a]) => `${f}(${a})`),
  fc.tuple(fc.stringMatching(/^[a-z_]{1,8}$/), fc.stringMatching(/^[a-z_]{1,8}$/)).map(([a, b]) => `${a} | ${b}`),
);
const variable = fc.oneof(
  fc.record({ source: fc.constantFrom('path', 'query', 'header', 'body', 'xpath', 'fake'), value: ident, pipe }),
  fc.record({ source: fc.constantFrom('uuid', 'now_ms', 'now_iso', 'seq'), value: fc.constant(''), pipe }),
  fc.record({ source: fc.constant('script'), value: fc.oneof(fc.constant(''), ident), pipe }),
);

// ── JSON ─────────────────────────────────────────────────────────────

// The raw content of a JSON string: what sits between its quotes, escapes included.
const jsonRaw = fc
  .string()
  .filter(noExpression)
  .map((s) => JSON.stringify(s).slice(1, -1));
const jsonLiteral = fc.oneof(
  fc.integer().map(String),
  fc.double({ noNaN: true, noDefaultInfinity: true }).map((d) => JSON.stringify(d)),
  fc.constantFrom('true', 'false', 'null'),
);
const jsonItem = fc.oneof(
  variable.chain((v) => fc.boolean().map((asNumber) => ({ ...v, asNumber }))),
  jsonRaw.map((value) => ({ source: 'fixed', value, pipe: '', asNumber: false })),
  jsonLiteral.map((value) => ({ source: 'fixed', value, pipe: '', asNumber: true })),
);
const jsonKey = jsonRaw.filter((k) => k.trim() === k && k !== '');
const { jsonField } = fc.letrec((tie) => ({
  jsonField: fc.oneof(
    { depthSize: 'small' },
    fc.tuple(jsonKey, jsonItem).map(([key, item]) => ({ key, fieldType: 'value', ...item })),
    fc.tuple(jsonKey, fc.array(tie('jsonField'), { maxLength: 3 })).map(([key, children]) => ({
      key,
      fieldType: 'object',
      children,
    })),
    fc.tuple(jsonKey, fc.array(jsonItem, { maxLength: 3 })).map(([key, items]) => ({
      key,
      fieldType: 'array-values',
      items,
    })),
    fc.tuple(jsonKey, fc.array(tie('jsonField'), { maxLength: 3 })).map(([key, template]) => ({
      key,
      fieldType: 'array-objects',
      template,
    })),
  ),
}));

// What the builder by example accepts: objects of scalars, objects, arrays of scalars and arrays of one object. Its keys
// are the builder's rows: trimmed, and a blank one is a row not filled in yet (see exampleJsonToFields).
const exampleKey = fc.string().filter((k) => noExpression(k) && k !== '__proto__' && k.trim() === k && k !== '');
const scalar = fc.oneof(
  fc.string().filter(noExpression),
  fc.integer(),
  fc.double({ noNaN: true, noDefaultInfinity: true }).filter((d) => !Object.is(d, -0)),
  fc.boolean(),
  fc.constant(null),
);
const { jsonExample } = fc.letrec((tie) => ({
  jsonExample: fc.dictionary(
    exampleKey,
    fc.oneof(
      { depthSize: 'small' },
      scalar,
      tie('jsonExample'),
      fc.array(scalar, { maxLength: 3 }),
      tie('jsonExample').map((o) => [o]),
    ),
    { maxKeys: 4 },
  ),
}));

describe('tpl-utils properties (JSON)', () => {
  it('fields written as a template read back as the same fields', () => {
    fc.assert(
      fc.property(fc.array(jsonField, { maxLength: 4 }), (fields) => {
        const template = fieldsToTemplate(fields);
        expect(validateTemplateAsJson(template)).toBeNull();
        expect(templateToFields(template)).toEqual(fields);
      }),
    );
  });

  it('a pasted example becomes a template that is the example itself', () => {
    fc.assert(
      fc.property(jsonExample, (example) => {
        const template = fieldsToTemplate(exampleJsonToFields(example));
        expect(validateTemplateAsJson(template)).toBeNull();
        expect(JSON.parse(templateToTestJson(template))).toEqual(example);
      }),
    );
  });

  it('any text either reads as fields or is refused with a message, never with a crash', () => {
    fc.assert(
      fc.property(fc.oneof(fc.string(), fc.json()), (text) => {
        try {
          expect(Array.isArray(templateToFields(text))).toBe(true);
        } catch (error) {
          expect(error instanceof SyntaxError || error instanceof TypeError).toBe(true);
          if (error instanceof TypeError)
            expect(error.message).toBe('The JSON must be an object to be shown in the guided view.');
        }
      }),
    );
  });
});

// ── XML ──────────────────────────────────────────────────────────────

// XML 1.0 text without the characters it forbids, nor the carriage returns and edge spaces a parser does not keep.
const xmlChars = fc
  .string({ unit: 'grapheme' })
  .filter((s) => noExpression(s) && s.trim() === s && !/[\u0000-\u0008\u000B\u000C\u000E-\u001F\r￾￿]/.test(s));
const escapeText = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const escapeAttr = (s) => escapeText(s).replace(/"/g, '&quot;');
const tag = fc.stringMatching(/^[a-z][a-z0-9]{0,6}$/);
const attributes = (valueRaw) =>
  fc.uniqueArray(
    fc.record({ name: fc.stringMatching(/^[a-z][a-z0-9]{0,5}$/) }).chain(({ name }) =>
      fc.oneof(
        variable.map((v) => ({ name, ...v })),
        valueRaw.map((value) => ({ name, source: 'fixed', value, pipe: '' })),
      ),
    ),
    { selector: (a) => a.name, maxLength: 2 },
  );
const xmlTextRaw = xmlChars.map(escapeText);
const xmlAttrRaw = xmlChars.filter((s) => !/[\t\n]/.test(s)).map(escapeAttr);
const { xmlField } = fc.letrec((tie) => ({
  xmlField: fc.oneof(
    { depthSize: 'small' },
    fc
      .tuple(
        tag,
        attributes(xmlAttrRaw),
        fc.oneof(
          variable,
          xmlTextRaw.map((value) => ({ source: 'fixed', value, pipe: '' })),
        ),
      )
      .map(([t, attrs, leaf]) => ({ tag: t, nodeType: 'value', attributes: attrs, ...leaf })),
    fc
      .tuple(tag, attributes(xmlAttrRaw), fc.array(tie('xmlField'), { minLength: 1, maxLength: 3 }))
      .map(([t, attrs, children]) => ({ tag: t, nodeType: 'parent', attributes: attrs, children })),
  ),
}));

// A pasted XML example, built with its own escaping, and the same document as a plain tree to compare with.
const { xmlNode } = fc.letrec((tie) => ({
  xmlNode: fc.oneof(
    { depthSize: 'small' },
    fc.record({
      tag,
      attrs: fc.uniqueArray(
        fc.tuple(
          fc.stringMatching(/^[a-z][a-z0-9]{0,5}$/),
          xmlChars.filter((s) => !/[\t\n]/.test(s)),
        ),
        { selector: ([n]) => n, maxLength: 2 },
      ),
      text: xmlChars,
    }),
    fc.record({
      tag,
      attrs: fc.uniqueArray(
        fc.tuple(
          fc.stringMatching(/^[a-z][a-z0-9]{0,5}$/),
          xmlChars.filter((s) => !/[\t\n]/.test(s)),
        ),
        { selector: ([n]) => n, maxLength: 2 },
      ),
      children: fc.array(tie('xmlNode'), { minLength: 1, maxLength: 3 }),
    }),
  ),
}));
const serialize = (node) => {
  const attrs = node.attrs.map(([n, v]) => ` ${n}="${escapeAttr(v)}"`).join('');
  const inner = node.children ? node.children.map(serialize).join('') : escapeText(node.text);
  return `<${node.tag}${attrs}>${inner}</${node.tag}>`;
};
// The document a text parses to, as a plain tree: tags, attributes by name, leaf text.
const tree = (xml) => {
  const doc = new DOMParser().parseFromString(xml, 'application/xml');
  expect(doc.querySelector('parsererror')).toBeNull();
  const walk = (el) => ({
    tag: el.tagName,
    attrs: Object.fromEntries(Array.from(el.attributes, (a) => [a.name, a.value])),
    ...(el.children.length ? { children: Array.from(el.children, walk) } : { text: el.textContent }),
  });
  return walk(doc.documentElement);
};

describe('tpl-utils properties (XML)', () => {
  it('fields written as a template read back as the same fields', () => {
    fc.assert(
      fc.property(
        tag,
        attributes(xmlAttrRaw),
        fc.array(xmlField, { minLength: 1, maxLength: 3 }),
        (rootTag, rootAttributes, fields) => {
          const parsed = templateToXmlFields(xmlFieldsToTemplate(fields, rootTag, rootAttributes));
          expect(parsed).toEqual({ rootTag, rootAttributes, fields });
        },
      ),
    );
  });

  it('a pasted example becomes a template that is the example itself', () => {
    fc.assert(
      fc.property(tag, fc.array(xmlNode, { minLength: 1, maxLength: 3 }), (rootTag, children) => {
        const example = serialize({ tag: rootTag, attrs: [], children });
        const { rootTag: root, rootAttributes, fields } = exampleXmlToFields(example);
        expect(tree(xmlFieldsToTemplate(fields, root, rootAttributes))).toEqual(tree(example));
      }),
    );
  });
});
