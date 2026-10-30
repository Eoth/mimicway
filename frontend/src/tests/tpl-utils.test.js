import { describe, it, expect } from 'vitest';
import {
  fieldsToTemplate,
  templateToTestJson,
  templateToFields,
  validateTemplateAsJson,
  validateTemplateAsXml,
  templateToPreview,
  buildExpr,
  varNameToSource,
  xmlFieldsToTemplate,
  exampleJsonToFields,
  exampleXmlToFields,
  templateToXmlFields,
} from '../lib/tpl-utils.js';

// ── fieldsToTemplate ─────────────────────────────────────────────────

describe('fieldsToTemplate', () => {
  it('empty fields produce empty object', () => {
    expect(fieldsToTemplate([])).toBe('{}');
  });

  it('single fixed string field', () => {
    const fields = [{ key: 'name', fieldType: 'value', source: 'fixed', value: 'Alice', pipe: '', asNumber: false }];
    expect(fieldsToTemplate(fields)).toBe('{"name":"Alice"}');
  });

  it('single numeric field', () => {
    const fields = [{ key: 'count', fieldType: 'value', source: 'fixed', value: '42', pipe: '', asNumber: true }];
    expect(fieldsToTemplate(fields)).toBe('{"count":42}');
  });

  it('variable with pipe', () => {
    const fields = [
      { key: 'siren', fieldType: 'value', source: 'path', value: 'siret', pipe: 'first(9)', asNumber: false },
    ];
    expect(fieldsToTemplate(fields)).toBe('{"siren":"{{path.siret | first(9)}}"}');
  });

  it('nested object', () => {
    const fields = [
      {
        key: 'data',
        fieldType: 'object',
        children: [{ key: 'id', fieldType: 'value', source: 'uuid', value: '', pipe: '', asNumber: false }],
      },
    ];
    const tpl = fieldsToTemplate(fields);
    expect(tpl).toBe('{"data":{"id":"{{uuid}}"}}');
  });

  it('array of values', () => {
    const fields = [
      {
        key: 'tags',
        fieldType: 'array-values',
        items: [
          { source: 'fixed', value: 'a', pipe: '', asNumber: false },
          { source: 'fixed', value: 'b', pipe: '', asNumber: false },
        ],
      },
    ];
    expect(fieldsToTemplate(fields)).toBe('{"tags":["a","b"]}');
  });

  it('array of objects', () => {
    const fields = [
      {
        key: 'items',
        fieldType: 'array-objects',
        template: [{ key: 'id', fieldType: 'value', source: 'seq', value: '', pipe: '', asNumber: true }],
      },
    ];
    expect(fieldsToTemplate(fields)).toBe('{"items":[{"id":{{seq}}}]}');
  });
});

// ── templateToTestJson ───────────────────────────────────────────────

describe('templateToTestJson', () => {
  it('passes normal JSON through', () => {
    expect(templateToTestJson('{"a":"b"}')).toBe('{"a":"b"}');
  });

  it('replaces {{var}} with placeholder', () => {
    expect(templateToTestJson('{"k":"{{path.x}}"}')).toBe('{"k":"__var__"}');
  });

  it('handles pipe inside variable', () => {
    expect(templateToTestJson('{"k":"{{path.x | first(9)}}"}')).toBe('{"k":"__var__"}');
  });

  it('handles nested objects', () => {
    const tpl = '{"a":{"b":"{{uuid}}"}}';
    const test = templateToTestJson(tpl);
    expect(test).toBe('{"a":{"b":"__var__"}}');
    expect(() => JSON.parse(test)).not.toThrow();
  });

  it('handles numbers without quotes', () => {
    expect(templateToTestJson('{"n":{{seq}}}')).toBe('{"n":"__var__"}');
  });

  it('preserves arrays', () => {
    const tpl = '{"a":["x","{{uuid}}"]}';
    const test = templateToTestJson(tpl);
    expect(test).toBe('{"a":["x","__var__"]}');
    expect(() => JSON.parse(test)).not.toThrow();
  });
});

// ── validateTemplateAsJson ───────────────────────────────────────────

describe('validateTemplateAsJson', () => {
  it('accepts valid template', () => {
    expect(validateTemplateAsJson('{"name":"{{path.siret}}"}')).toBeNull();
  });

  it('accepts empty template', () => {
    expect(validateTemplateAsJson('')).toBeNull();
    expect(validateTemplateAsJson('  ')).toBeNull();
  });

  it('accepts nested objects with pipes', () => {
    expect(validateTemplateAsJson('{"data":{"siren":"{{path.siret | first(9)}}"}}')).toBeNull();
  });

  it('rejects malformed JSON', () => {
    const err = validateTemplateAsJson('{"name":}');
    expect(err).not.toBeNull();
    expect(err).toMatch(/^Invalid JSON: ./);
  });

  it('rejects unbalanced braces', () => {
    const err = validateTemplateAsJson('{"name":"val"');
    expect(err).not.toBeNull();
  });

  it('accepts what fieldsToTemplate produces', () => {
    const fields = [
      { key: 'siret', fieldType: 'value', source: 'path', value: 'siret', pipe: '', asNumber: false },
      { key: 'siren', fieldType: 'value', source: 'path', value: 'siret', pipe: 'first(9)', asNumber: false },
      {
        key: 'data',
        fieldType: 'object',
        children: [
          { key: 'nom', fieldType: 'value', source: 'fake', value: 'CompanyName', pipe: '', asNumber: false },
          { key: 'ts', fieldType: 'value', source: 'now_ms', value: '', pipe: '', asNumber: true },
        ],
      },
    ];
    const tpl = fieldsToTemplate(fields);
    expect(validateTemplateAsJson(tpl)).toBeNull();
  });
});

// ── Round-trip: Fields → Template → Fields ───────────────────────────

describe('round-trip: fields -> template -> fields', () => {
  function roundTrip(fields) {
    const tpl = fieldsToTemplate(fields);
    const err = validateTemplateAsJson(tpl);
    expect(err).toBeNull();
    const back = templateToFields(tpl);
    return back;
  }

  it('simple fixed value', () => {
    const fields = [{ key: 'name', fieldType: 'value', source: 'fixed', value: 'Alice', pipe: '', asNumber: false }];
    const back = roundTrip(fields);
    expect(back).toHaveLength(1);
    expect(back[0].key).toBe('name');
    expect(back[0].source).toBe('fixed');
    expect(back[0].value).toBe('Alice');
  });

  it('variable with pipe', () => {
    const fields = [
      { key: 'siren', fieldType: 'value', source: 'path', value: 'siret', pipe: 'first(9)', asNumber: false },
    ];
    const back = roundTrip(fields);
    expect(back[0].source).toBe('path');
    expect(back[0].value).toBe('siret');
    expect(back[0].pipe).toBe('first(9)');
  });

  it('nested object', () => {
    const fields = [
      {
        key: 'data',
        fieldType: 'object',
        children: [
          { key: 'id', fieldType: 'value', source: 'uuid', value: '', pipe: '', asNumber: false },
          { key: 'name', fieldType: 'value', source: 'fake', value: 'FirstName', pipe: '', asNumber: false },
        ],
      },
    ];
    const back = roundTrip(fields);
    expect(back[0].fieldType).toBe('object');
    expect(back[0].children).toHaveLength(2);
    expect(back[0].children[0].source).toBe('uuid');
    expect(back[0].children[1].source).toBe('fake');
    expect(back[0].children[1].value).toBe('FirstName');
  });

  it('multiple pipes chained', () => {
    const fields = [
      { key: 'v', fieldType: 'value', source: 'path', value: 'name', pipe: 'lower | first(5)', asNumber: false },
    ];
    const back = roundTrip(fields);
    expect(back[0].pipe).toBe('lower | first(5)');
  });

  it('numeric value without quotes', () => {
    const fields = [{ key: 'seq', fieldType: 'value', source: 'seq', value: '', pipe: '', asNumber: true }];
    const back = roundTrip(fields);
    expect(back[0].asNumber).toBe(true);
    expect(back[0].source).toBe('seq');
  });

  it('full INSEE-like template', () => {
    const fields = [
      { key: 'siret', fieldType: 'value', source: 'path', value: 'siret', pipe: '', asNumber: false },
      { key: 'siren', fieldType: 'value', source: 'path', value: 'siret', pipe: 'first(9)', asNumber: false },
      {
        key: 'unite_legale',
        fieldType: 'object',
        children: [
          { key: 'denomination', fieldType: 'value', source: 'fake', value: 'CompanyName', pipe: '', asNumber: false },
          {
            key: 'adresse',
            fieldType: 'object',
            children: [
              { key: 'ville', fieldType: 'value', source: 'fake', value: 'CityFR', pipe: '', asNumber: false },
            ],
          },
        ],
      },
      {
        key: 'meta',
        fieldType: 'object',
        children: [
          { key: 'timestamp', fieldType: 'value', source: 'now_ms', value: '', pipe: '', asNumber: true },
          { key: 'seq', fieldType: 'value', source: 'seq', value: '', pipe: '', asNumber: true },
        ],
      },
    ];
    const back = roundTrip(fields);
    expect(back).toHaveLength(4);
    expect(back[0].source).toBe('path');
    expect(back[1].pipe).toBe('first(9)');
    expect(back[2].fieldType).toBe('object');
    expect(back[2].children[1].fieldType).toBe('object');
    expect(back[3].children[0].asNumber).toBe(true);
  });
});

// ── templateToFields error cases ─────────────────────────────────────

describe('round-trip: arrays holding expressions with quotes and brackets', () => {
  it('reads each item of an array whole, past the quotes and brackets of its expression', () => {
    const tpl = '{"tags":["{{query.x | default("]")}}","b"]}';
    expect(templateToFields(tpl)).toEqual([
      {
        key: 'tags',
        fieldType: 'array-values',
        items: [
          { source: 'query', value: 'x', pipe: 'default("]")', asNumber: false },
          { source: 'fixed', value: 'b', pipe: '', asNumber: false },
        ],
      },
    ]);
  });
});

describe('templateToFields error cases', () => {
  it('throws on array root', () => {
    expect(() => templateToFields('[1,2,3]')).toThrow();
  });

  it('throws on malformed JSON', () => {
    expect(() => templateToFields('{"a":}')).toThrow();
  });

  it('returns empty for empty object', () => {
    expect(templateToFields('{}')).toEqual([]);
  });
});

// ── templateToPreview ────────────────────────────────────────────────

describe('templateToPreview', () => {
  it('renders variables with angle brackets', () => {
    const result = templateToPreview('{"name":"{{path.x}}"}');
    expect(result).toBe('{"name":"«{{path.x}}»"}');
  });

  it('renders nested objects correctly', () => {
    const result = templateToPreview('{"a":{"b":"v"}}');
    expect(result).toBe('{"a":{"b":"v"}}');
  });
});

// ── validateTemplateAsXml ────────────────────────────────────────────

describe('validateTemplateAsXml', () => {
  it('accepts valid XML template', () => {
    expect(validateTemplateAsXml('<root><id>{{uuid}}</id></root>')).toBeNull();
  });

  it('rejects invalid XML', () => {
    const err = validateTemplateAsXml('<root><unclosed>');
    expect(err).not.toBeNull();
  });

  it('accepts empty', () => {
    expect(validateTemplateAsXml('')).toBeNull();
  });
});

// ── buildExpr ────────────────────────────────────────────────────────

describe('buildExpr', () => {
  it('fixed returns value directly', () => {
    expect(buildExpr({ source: 'fixed', value: 'hello' })).toBe('hello');
  });

  it('path without pipe', () => {
    expect(buildExpr({ source: 'path', value: 'id', pipe: '' })).toBe('{{path.id}}');
  });

  it('path with pipe', () => {
    expect(buildExpr({ source: 'path', value: 'siret', pipe: 'first(9)' })).toBe('{{path.siret | first(9)}}');
  });

  it('uuid', () => {
    expect(buildExpr({ source: 'uuid', value: '', pipe: '' })).toBe('{{uuid}}');
  });

  it('script', () => {
    expect(buildExpr({ source: 'script', value: '', pipe: '' })).toBe('{{script}}');
  });

  it('script with field', () => {
    expect(buildExpr({ source: 'script', value: 'result', pipe: '' })).toBe('{{script.result}}');
  });

  it('xpath without pipe', () => {
    expect(buildExpr({ source: 'xpath', value: 'Envelope/Body/recherche/Siret', pipe: '' })).toBe(
      '{{xpath.Envelope/Body/recherche/Siret}}',
    );
  });

  it('xpath with pipe', () => {
    expect(buildExpr({ source: 'xpath', value: 'Envelope/Body/recherche/Siret', pipe: 'substr(0,9)' })).toBe(
      '{{xpath.Envelope/Body/recherche/Siret | substr(0,9)}}',
    );
  });
});

// ── varNameToSource ──────────────────────────────────────────────────

describe('varNameToSource', () => {
  it('parses path', () => {
    expect(varNameToSource('path.siret')).toEqual({ source: 'path', value: 'siret' });
  });
  it('parses uuid', () => {
    expect(varNameToSource('uuid')).toEqual({ source: 'uuid', value: '' });
  });
  it('parses fake', () => {
    expect(varNameToSource('fake.Email')).toEqual({ source: 'fake', value: 'Email' });
  });
  it('parses script', () => {
    expect(varNameToSource('script')).toEqual({ source: 'script', value: '' });
  });
  it('parses script.field', () => {
    expect(varNameToSource('script.result')).toEqual({ source: 'script', value: 'result' });
  });
  it('unknown falls back to fixed', () => {
    expect(varNameToSource('unknown')).toEqual({ source: 'fixed', value: 'unknown' });
  });
  it('parses xpath (round-trip with a slash-containing path)', () => {
    expect(varNameToSource('xpath.Envelope/Body/recherche/Siret')).toEqual({
      source: 'xpath',
      value: 'Envelope/Body/recherche/Siret',
    });
  });
});

// ── xmlFieldsToTemplate ──────────────────────────────────────────────

describe('xmlFieldsToTemplate', () => {
  it('empty fields produce empty root', () => {
    expect(xmlFieldsToTemplate([])).toBe('<response></response>');
  });

  it('custom root tag', () => {
    expect(xmlFieldsToTemplate([], 'data')).toBe('<data></data>');
  });

  it('single value node', () => {
    const fields = [{ tag: 'id', nodeType: 'value', source: 'fixed', value: '42', pipe: '' }];
    expect(xmlFieldsToTemplate(fields)).toBe('<response><id>42</id></response>');
  });

  it('variable with pipe', () => {
    const fields = [{ tag: 'siren', nodeType: 'value', source: 'path', value: 'siret', pipe: 'first(9)' }];
    expect(xmlFieldsToTemplate(fields)).toBe('<response><siren>{{path.siret | first(9)}}</siren></response>');
  });

  it('nested parent node', () => {
    const fields = [
      {
        tag: 'data',
        nodeType: 'parent',
        children: [
          { tag: 'id', nodeType: 'value', source: 'uuid', value: '', pipe: '' },
          { tag: 'name', nodeType: 'value', source: 'fake', value: 'FirstName', pipe: '' },
        ],
      },
    ];
    const tpl = xmlFieldsToTemplate(fields, 'root');
    expect(tpl).toBe('<root><data><id>{{uuid}}</id><name>{{fake.FirstName}}</name></data></root>');
  });

  it('what xmlFieldsToTemplate produces is valid XML', () => {
    const fields = [
      { tag: 'id', nodeType: 'value', source: 'uuid', value: '', pipe: '' },
      {
        tag: 'info',
        nodeType: 'parent',
        children: [{ tag: 'city', nodeType: 'value', source: 'fake', value: 'CityFR', pipe: 'upper' }],
      },
    ];
    const tpl = xmlFieldsToTemplate(fields, 'resp');
    expect(validateTemplateAsXml(tpl)).toBeNull();
  });
});

// ── XML validation edge cases ────────────────────────────────────────

describe('validateTemplateAsXml edge cases', () => {
  it('accepts variables with pipes inside tags', () => {
    expect(validateTemplateAsXml('<r><v>{{path.x | upper}}</v></r>')).toBeNull();
  });

  it('accepts multiple variables in same tag', () => {
    expect(validateTemplateAsXml('<r>{{fake.FirstName}} {{fake.LastName}}</r>')).toBeNull();
  });

  it('rejects mismatched tags', () => {
    expect(validateTemplateAsXml('<a><b></a>')).not.toBeNull();
  });

  it('rejects empty tag names', () => {
    expect(validateTemplateAsXml('<></>')).not.toBeNull();
  });
});

// ── exampleJsonToFields ("paste an example" view) ────────────────────

describe('exampleJsonToFields', () => {
  it('turns a flat object into fixed fields', () => {
    const fields = exampleJsonToFields({ siret: '44306184100047', nom: 'ACME Corp' });
    expect(fields).toEqual([
      { key: 'siret', fieldType: 'value', source: 'fixed', value: '44306184100047', pipe: '', asNumber: false },
      { key: 'nom', fieldType: 'value', source: 'fixed', value: 'ACME Corp', pipe: '', asNumber: false },
    ]);
  });

  it('marks numbers and booleans with asNumber', () => {
    const fields = exampleJsonToFields({ age: 42, actif: true });
    expect(fields[0].asNumber).toBe(true);
    expect(fields[0].value).toBe('42');
    expect(fields[1].asNumber).toBe(true);
  });

  it('turns a nested object into an object field', () => {
    const fields = exampleJsonToFields({ adresse: { ville: 'Paris', cp: '75001' } });
    expect(fields[0].fieldType).toBe('object');
    expect(fields[0].children).toEqual([
      { key: 'ville', fieldType: 'value', source: 'fixed', value: 'Paris', pipe: '', asNumber: false },
      { key: 'cp', fieldType: 'value', source: 'fixed', value: '75001', pipe: '', asNumber: false },
    ]);
  });

  it('turns an array of scalars into array-values', () => {
    const fields = exampleJsonToFields({ tags: ['a', 'b'] });
    expect(fields[0].fieldType).toBe('array-values');
    expect(fields[0].items).toEqual([
      { source: 'fixed', value: 'a', pipe: '', asNumber: false },
      { source: 'fixed', value: 'b', pipe: '', asNumber: false },
    ]);
  });

  it('turns an array of objects into array-objects, its template taken from the first element', () => {
    const fields = exampleJsonToFields({ items: [{ id: 1 }, { id: 2 }] });
    expect(fields[0].fieldType).toBe('array-objects');
    expect(fields[0].template).toEqual([
      { key: 'id', fieldType: 'value', source: 'fixed', value: '1', pipe: '', asNumber: true },
    ]);
  });

  it('handles an array of nulls without throwing', () => {
    expect(() => exampleJsonToFields({ items: [null, null] })).not.toThrow();
    const fields = exampleJsonToFields({ items: [null, null] });
    expect(fields[0].fieldType).toBe('array-values');
  });

  it('rejects a root that is not an object (array, scalar or null)', () => {
    expect(() => exampleJsonToFields([1, 2, 3])).toThrow(TypeError);
    expect(() => exampleJsonToFields('just a string')).toThrow(TypeError);
    expect(() => exampleJsonToFields(null)).toThrow(TypeError);
  });

  it('gives fields that fieldsToTemplate turns into a valid template', () => {
    const fields = exampleJsonToFields({ siret: '123', nested: { x: 1 } });
    const tpl = fieldsToTemplate(fields);
    expect(validateTemplateAsJson(tpl)).toBeNull();
  });
});

// ── xmlFieldsToTemplate — attributes (XML "paste an example" view) ────

describe('xmlFieldsToTemplate with attributes', () => {
  it('renders the attributes of the root', () => {
    const tpl = xmlFieldsToTemplate([], 'response', [
      { name: 'xmlns:soap', source: 'fixed', value: 'http://schemas.xmlsoap.org/soap/', pipe: '' },
    ]);
    expect(tpl).toBe('<response xmlns:soap="http://schemas.xmlsoap.org/soap/"></response>');
  });

  it('renders the attributes of a value node, a variable with a pipe included', () => {
    const fields = [
      {
        tag: 'id',
        nodeType: 'value',
        source: 'fixed',
        value: '42',
        pipe: '',
        attributes: [{ name: 'type', source: 'path', value: 'kind', pipe: 'upper' }],
      },
    ];
    expect(xmlFieldsToTemplate(fields)).toBe('<response><id type="{{path.kind | upper}}">42</id></response>');
  });

  it('renders the attributes of a parent node', () => {
    const fields = [
      {
        tag: 'client',
        nodeType: 'parent',
        attributes: [{ name: 'id', source: 'fixed', value: '7', pipe: '' }],
        children: [{ tag: 'nom', nodeType: 'value', source: 'fixed', value: 'ACME', pipe: '' }],
      },
    ];
    expect(xmlFieldsToTemplate(fields)).toBe('<response><client id="7"><nom>ACME</nom></client></response>');
  });

  it('renders a field and a root without `attributes` with no attributes', () => {
    const fields = [{ tag: 'id', nodeType: 'value', source: 'uuid', value: '', pipe: '' }];
    expect(xmlFieldsToTemplate(fields, 'root')).toBe('<root><id>{{uuid}}</id></root>');
  });
});

// ── exampleXmlToFields (XML "paste an example" view) ─────────────────

describe('exampleXmlToFields', () => {
  it('turns flat XML into fixed fields and finds the root tag', () => {
    const { rootTag, rootAttributes, fields } = exampleXmlToFields(
      '<response><siret>44306184100047</siret><nom>ACME Corp</nom></response>',
    );
    expect(rootTag).toBe('response');
    expect(rootAttributes).toEqual([]);
    expect(fields).toEqual([
      { tag: 'siret', nodeType: 'value', source: 'fixed', value: '44306184100047', pipe: '', attributes: [] },
      { tag: 'nom', nodeType: 'value', source: 'fixed', value: 'ACME Corp', pipe: '', attributes: [] },
    ]);
  });

  it('turns a nested element into a parent node', () => {
    const { fields } = exampleXmlToFields('<r><adresse><ville>Paris</ville><cp>75001</cp></adresse></r>');
    expect(fields[0].nodeType).toBe('parent');
    expect(fields[0].children).toEqual([
      { tag: 'ville', nodeType: 'value', source: 'fixed', value: 'Paris', pipe: '', attributes: [] },
      { tag: 'cp', nodeType: 'value', source: 'fixed', value: '75001', pipe: '', attributes: [] },
    ]);
  });

  it('keeps sibling elements with the same tag as separate children (no list type, unlike JSON)', () => {
    const { fields } = exampleXmlToFields('<r><items><item>A</item><item>B</item></items></r>');
    expect(fields[0].children).toHaveLength(2);
    expect(fields[0].children[0].value).toBe('A');
    expect(fields[0].children[1].value).toBe('B');
  });

  it('finds the attributes of the root and of a child', () => {
    const { rootAttributes, fields } = exampleXmlToFields(
      '<response xmlns:soap="http://x" ver="1"><id type="uuid">42</id></response>',
    );
    expect(rootAttributes).toEqual([
      { name: 'xmlns:soap', source: 'fixed', value: 'http://x', pipe: '' },
      { name: 'ver', source: 'fixed', value: '1', pipe: '' },
    ]);
    expect(fields[0].attributes).toEqual([{ name: 'type', source: 'fixed', value: 'uuid', pipe: '' }]);
  });

  it('keeps namespace prefixes as literal text, without resolving them', () => {
    const { rootTag, fields } = exampleXmlToFields(
      '<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/"><soap:Body><getClientResponse><nom>ACME</nom></getClientResponse></soap:Body></soap:Envelope>',
    );
    expect(rootTag).toBe('soap:Envelope');
    expect(fields[0].tag).toBe('soap:Body');
    expect(fields[0].children[0].tag).toBe('getClientResponse');
  });

  it('rejects invalid XML with a TypeError', () => {
    expect(() => exampleXmlToFields('<a><b></a>')).toThrow(TypeError);
  });

  it('rejects a root that holds only text, no nested element', () => {
    expect(() => exampleXmlToFields('<response>just text</response>')).toThrow(
      'The XML root holds no nested element. Paste XML with at least one child element.',
    );
  });

  it('rejects an empty string', () => {
    expect(() => exampleXmlToFields('   ')).toThrow(TypeError);
  });

  it('gives fields that xmlFieldsToTemplate turns back into the same, valid XML', () => {
    const { rootTag, rootAttributes, fields } = exampleXmlToFields(
      '<resp ver="1"><client id="7"><nom>ACME</nom></client></resp>',
    );
    const tpl = xmlFieldsToTemplate(fields, rootTag, rootAttributes);
    expect(validateTemplateAsXml(tpl)).toBeNull();
    expect(tpl).toBe('<resp ver="1"><client id="7"><nom>ACME</nom></client></resp>');
  });
});

// ── templateToXmlFields ──────────────────────────────────────────────
// Unlike exampleXmlToFields, which reads a literal XML sample, it reads back a template rendered by
// xmlFieldsToTemplate, `{{expr | pipe}}` included: RuleResponseSection.svelte uses it to restore the XML view when a
// rule is reopened.

describe('templateToXmlFields', () => {
  it('reads a literal value back as a "fixed" source', () => {
    const { rootTag, fields } = templateToXmlFields('<response><nom>ACME</nom></response>');
    expect(rootTag).toBe('response');
    expect(fields).toEqual([
      { tag: 'nom', nodeType: 'value', attributes: [], source: 'fixed', value: 'ACME', pipe: '' },
    ]);
  });

  it('reads back an {{expr}} expression without a pipe', () => {
    const { fields } = templateToXmlFields('<response><siret>{{path.siret}}</siret></response>');
    expect(fields[0].source).toBe('path');
    expect(fields[0].value).toBe('siret');
    expect(fields[0].pipe).toBe('');
  });

  it('reads back an {{expr | pipe}} expression with its pipe', () => {
    const { fields } = templateToXmlFields('<response><siret>{{path.siret | upper}}</siret></response>');
    expect(fields[0].source).toBe('path');
    expect(fields[0].value).toBe('siret');
    expect(fields[0].pipe).toBe('upper');
  });

  it('does not split a pipe at a | inside its parentheses', () => {
    const { fields } = templateToXmlFields('<response><n>{{query.n | default("x|y")}}</n></response>');
    expect(fields[0].pipe).toBe('default("x|y")');
  });

  it('reads back a nested parent node, recursively', () => {
    const { fields } = templateToXmlFields(
      '<response><client><nom>ACME</nom><siret>{{path.siret}}</siret></client></response>',
    );
    expect(fields[0].nodeType).toBe('parent');
    expect(fields[0].children).toHaveLength(2);
    expect(fields[0].children[1].source).toBe('path');
  });

  it('reads back the attributes of the root and of a node, each with its own source and pipe', () => {
    const { rootAttributes, fields } = templateToXmlFields(
      '<response ver="1"><id type="{{path.kind | upper}}">42</id></response>',
    );
    expect(rootAttributes).toEqual([{ name: 'ver', source: 'fixed', value: '1', pipe: '' }]);
    expect(fields[0].attributes[0]).toEqual({ name: 'type', source: 'path', value: 'kind', pipe: 'upper' });
  });

  it('round-trip templateToXmlFields -> xmlFieldsToTemplate gives back the same template', () => {
    const original =
      '<devisResponse ver="2"><client><nom>ACME</nom><siret>{{path.siret | upper}}</siret></client></devisResponse>';
    const { rootTag, rootAttributes, fields } = templateToXmlFields(original);
    expect(xmlFieldsToTemplate(fields, rootTag, rootAttributes)).toBe(original);
  });

  it('rejects an empty template', () => {
    expect(() => templateToXmlFields('   ')).toThrow(TypeError);
  });

  it('rejects an invalid XML template', () => {
    expect(() => templateToXmlFields('<a><b></a>')).toThrow(TypeError);
  });

  it('rejects a template without a root element', () => {
    expect(() => templateToXmlFields('just text')).toThrow(TypeError);
  });
});

// ── Expressions the builders have no source for ─────────────────────
// A text in quotes transformed by a pipe (a date converted to epoch seconds), now_epoch, a name of an earlier version
// (pre_script, post_script): the builders offer no source for them, and reading a template back used to turn each into
// its bare name, which the next save wrote as plain text, the expression lost.

describe('templates that the builders read back without a source', () => {
  const unmodelled = [
    '{{"2026-12-31T10:00:00Z" | epoch_seconds}}',
    '{{now_epoch}}',
    '{{pre_script.total | default("0")}}',
    '{{post_script}}',
  ];

  it('reads an expression whose quotes sit inside a JSON string', () => {
    const tpl = '{"a":"{{query.x | default("none")}}","b":"{{"2026-12-31" | date("dd/MM/yyyy")}}"}';
    expect(fieldsToTemplate(templateToFields(tpl))).toBe(tpl);
    expect(templateToFields(tpl)[0]).toMatchObject({ source: 'query', value: 'x', pipe: 'default("none")' });
  });

  it('keeps each expression whole through the JSON builder', () => {
    for (const expr of unmodelled) {
      const tpl = `{"a":${expr},"b":"${expr}"}`;
      expect(fieldsToTemplate(templateToFields(tpl)), expr).toBe(tpl);
    }
  });

  it('keeps each expression whole through the XML builder', () => {
    for (const expr of unmodelled) {
      const tpl = `<response><a>${expr}</a></response>`;
      const { rootTag, rootAttributes, fields } = templateToXmlFields(tpl);
      expect(xmlFieldsToTemplate(fields, rootTag, rootAttributes), expr).toBe(tpl);
    }
  });
});
