// The screens the translation tests render, with the data and the clicks that open each of their states. The
// pseudo-locale test (l10n.test.js) proves on them that no visible word escapes t; the French test shows them in
// French. One list, so that both cover the same states of the interface.
import { fireEvent, waitFor } from '@testing-library/svelte';
import { expect, vi } from 'vitest';

export const group = { name: 'g1', code: 'c0d3e', admins: ['u1'], members: ['u2'] };
export const proxied = {
  name: 'a1',
  listen_path: '/v1/{n}',
  real_target_url: 'http://10.0.0.1:8080',
  is_mocked: false,
  rewrite_directory_urls: false,
  group_name: 'g1',
  wsdl_mode: 'auto',
  rules: [],
};
export const fullRule = {
  name: 'r1',
  method: 'POST',
  sub_path: '/x1',
  action: 'mock',
  pre_script: '1',
  script: '2',
  post_script: '3',
  response_mode: 'advanced',
  conditions: {
    all_of: [{ source: { type: 'QueryParam', key: 'q' }, operator: { type: 'Eq', value: '1' } }],
    any_of: [{ source: { type: 'BodyRaw' }, operator: { type: 'Exists' } }],
  },
  response: {
    status: 200,
    headers: [{ name: 'Content-Type', value: 'application/json' }],
    body: [
      { type: 'Template', template: '{}' },
      { type: 'Literal', value: '1' },
      { type: 'Uuid' },
      { type: 'PickFrom', values: ['1', '2'] },
      { type: 'FakeData', kind: { type: 'FirstName' } },
      { type: 'PathSegment', index: 1 },
    ],
    chaos: { delay_ms: 1, delay_min_ms: null, delay_max_ms: null, error_rate: 0.1, error_status: 500 },
  },
};
export const mocked = { ...proxied, name: 'b2', is_mocked: true, group_name: null, rules: [fullRule] };
const captured = {
  remaining_path: '/x1',
  path_params: {},
  query_params: { q: '1' },
  headers: {},
  body: '{}',
  body_truncated: true,
  content_type: null,
};
const logs = [
  {
    timestamp: 1,
    service_name: 'b2',
    group_name: null,
    method: 'POST',
    path: '/b2/x1',
    mode: 'mock',
    rule_matched: 'r1',
    target_url: null,
    status: 200,
    captured,
  },
  {
    timestamp: 2,
    service_name: 'a1',
    group_name: 'g1',
    method: 'GET',
    path: '/c0d3e/a1/x2',
    mode: 'proxy',
    rule_matched: null,
    target_url: 'http://10.0.0.1:8080/x2',
    status: 502,
    captured: null,
  },
];

/**
 * Gives the API client, mocked by the test file with `vi.mock('../lib/api.js')` (every function becomes a `vi.fn()`),
 * the answers the screens show, and jsdom the media query the theme reads.
 */
export function answerApi(api) {
  window.matchMedia = window.matchMedia || vi.fn().mockReturnValue({ matches: false });
  api.getAuthStatus.mockResolvedValue({ enabled: false, show_reset_button: true });
  api.getMessagingStatus.mockResolvedValue({ available: true });
  api.getTcpStatus.mockResolvedValue([{ name: 't1', listen_port: 9000, listening: true, error: null }]);
  api.getTcpServices.mockResolvedValue([
    { name: 't1', listen_port: 9000, rules: [{ name: 'r1', matcher: { type: 'Any' }, response_hex: '' }] },
  ]);
  api.getServices.mockResolvedValue([proxied, mocked]);
  api.getGroups.mockResolvedValue([group]);
  api.getBackups.mockResolvedValue([{ filename: 'f1.yaml', created_at_ms: 1, size_bytes: 2048, protected: true }]);
  api.getLogs.mockResolvedValue(logs);
  api.getMessagingLogs.mockResolvedValue([
    {
      timestamp: 1,
      direction: 'out',
      topic: 't1',
      service_name: 'b2',
      rule_matched: 'r1',
      matched: true,
      body_preview: '{}',
      body_truncated: true,
      body_size_bytes: 9,
    },
  ]);
  api.getObservationStatus.mockResolvedValue([{ group_name: 'g1', service_name: 'a1' }]);
  api.getServiceSuggestions.mockResolvedValue([
    {
      outcome: 'Conditional',
      rules: [
        {
          method: 'GET',
          sub_path: '/x2',
          condition: { source: { type: 'QueryParam', key: 'q' }, operator: { type: 'Eq', value: '1' } },
          response: { status: 200, headers: [], body: [{ type: 'Literal', value: '1' }] },
          sample_count: 3,
        },
      ],
    },
    {
      outcome: 'Unconditional',
      rule: {
        method: 'GET',
        sub_path: '/x3',
        condition: null,
        response: { status: 404, headers: [], body: [] },
        sample_count: 3,
      },
    },
    { outcome: 'VarianceUnexplained', sample_count: 4, response_class_count: 2 },
  ]);
  api.testRule.mockResolvedValue({
    overall_matched: false,
    method_matches: true,
    sub_path_matches: false,
    body_truncated: true,
    all_of: [{ condition: fullRule.conditions.all_of[0], matched: false, found_value: null, hint: null }],
    any_of: [{ condition: fullRule.conditions.any_of[0], matched: true, found_value: '1', hint: null }],
    script_errors: [{ slot: 'pre_script', message: '1' }],
    script_results: [{ slot: 'script', value: '1', fields: { k1: '2' } }],
  });
}

// Code, data marked translate="no" (names, URLs, template expressions) and form values are not interface text.
const NOT_INTERFACE_TEXT = 'code, pre, textarea, datalist, script, style, kbd, [translate="no"]';

/**
 * The interface text of `container`, as { text, where }: the own text of each element, then the title, placeholder,
 * aria-label and alt attributes. The text of an element comes as a whole, its code children standing for
 * placeholders, since a translated sentence may be split around them (see Sentence.svelte); its other children cut
 * it into pieces.
 */
export function visibleTexts(container) {
  const texts = [];
  for (const element of [container, ...container.querySelectorAll('*')]) {
    if (element.closest(NOT_INTERFACE_TEXT)) continue;
    // An option showing its own value is an identifier (an HTTP method, a fake-data kind).
    if (element.tagName === 'OPTION' && element.textContent.trim() === element.value) continue;
    const SEPARATOR = '\u0000';
    const own = [...element.childNodes]
      .map((n) => {
        if (n.nodeType === Node.TEXT_NODE) return n.textContent;
        if (n.nodeType !== Node.ELEMENT_NODE) return ''; // the comments Svelte uses as anchors
        return n.matches(NOT_INTERFACE_TEXT) ? ' ' : SEPARATOR;
      })
      .join('');
    for (const piece of own.split(SEPARATOR)) texts.push({ text: piece, where: `<${element.tagName.toLowerCase()}>` });
  }
  for (const element of container.querySelectorAll('[title], [placeholder], [aria-label], [alt]')) {
    if (element.closest('[translate="no"]')) continue;
    for (const attribute of ['title', 'placeholder', 'aria-label', 'alt']) {
      const value = element.getAttribute(attribute);
      if (value) texts.push({ text: value, where: `${attribute} of <${element.tagName.toLowerCase()}>` });
    }
  }
  return texts;
}

const click = (container, testId) => fireEvent.click(container.querySelector(`[data-testid="${testId}"]`));
const appeared = (container, testId) =>
  waitFor(() => expect(container.querySelector(`[data-testid="${testId}"]`)).not.toBeNull());

const component = (name) => async () => (await import(`../../lib/components/${name}.svelte`)).default;

const jsonFields = [
  { key: 'k1', fieldType: 'value', source: 'fake', value: 'FirstName', pipe: '', asNumber: false },
  {
    key: 'k2',
    fieldType: 'object',
    children: [{ key: 'k3', fieldType: 'value', source: 'path', value: 'n', pipe: '', asNumber: false }],
  },
  { key: 'k4', fieldType: 'array-values', items: [{ source: 'fixed', value: '1', asNumber: true }] },
  { key: 'k5', fieldType: 'array-objects', template: [] },
];
const xmlFields = [
  {
    tag: 'k1',
    nodeType: 'value',
    source: 'query',
    value: 'q',
    pipe: '',
    attributes: [{ name: 'k6', source: 'fixed', value: '1' }],
  },
  {
    tag: 'k2',
    nodeType: 'parent',
    children: [{ tag: 'k3', nodeType: 'value', source: 'fake', value: 'FirstName', pipe: '' }],
  },
];

async function pasteInvalidSample(container, prefix, sample) {
  await fireEvent.input(container.querySelector(`[data-testid="${prefix}-paste-builder-textarea"]`), {
    target: { value: sample },
  });
  await click(container, `${prefix}-paste-builder-analyze-button`);
  expect(container.querySelector(`[data-testid="${prefix}-paste-builder-error"]`)).not.toBeNull();
}

/**
 * The screens, in the groups the tests report: each has an `id`, loads its `component`, renders it with `props`, then
 * `open` clicks its way to the state to check.
 */
export const SCREEN_GROUPS = [
  {
    name: 'the application shell, its list and its dialogs',
    screens: [
      {
        id: 'app',
        component: async () => (await import('../../App.svelte')).default,
        open: async (c) => {
          await appeared(c, 'app-add-service-button');
          await click(c, 'service-group-header-g1');
          await click(c, 'service-group-header-ungrouped');
        },
      },
    ],
  },
  {
    name: 'the service screens',
    screens: [
      { id: 'service-form', component: component('ServiceForm'), props: { availableGroups: [group] } },
      {
        id: 'service-detail-proxied',
        component: component('ServiceDetail'),
        props: { service: proxied, availableGroups: [group] },
        open: async (c) => {
          await appeared(c, 'observation-refresh-suggestions-button-a1');
          await click(c, 'observation-refresh-suggestions-button-a1');
          await appeared(c, 'observation-suggestion-a1-0-0');
        },
      },
      { id: 'service-detail-mocked', component: component('ServiceDetail'), props: { service: mocked } },
    ],
  },
  {
    name: 'the rule editor with every block open',
    screens: [
      {
        id: 'rule-form',
        component: component('RuleForm'),
        props: { rule: fullRule, serviceName: 'b2', listenPath: '/v1/{n}', existingRules: [] },
        open: async (c) => {
          await click(c, 'rule-form-advanced-options-toggle-button');
          await appeared(c, 'rule-tester-log-select');
          const select = c.querySelector('[data-testid="rule-tester-log-select"]');
          select.value = '0';
          await fireEvent.change(select);
          await click(c, 'rule-tester-test-button');
          await appeared(c, 'rule-tester-result');
          await click(c, 'rule-form-add-condition-allof-button');
        },
      },
    ],
  },
  {
    name: 'the response builders',
    screens: [
      { id: 'json-builder', component: component('JsonResponseBuilder'), props: { fields: jsonFields } },
      // Inside a nested field, the breadcrumb names the root.
      {
        id: 'json-builder-nested',
        component: component('JsonResponseBuilder'),
        props: { fields: jsonFields },
        open: (c) => click(c, 'json-builder-navigate-button-1'),
      },
      {
        id: 'json-builder-array-root',
        component: component('JsonResponseBuilder'),
        props: { fields: jsonFields, arrayRoot: true },
      },
      {
        id: 'json-paste',
        component: component('JsonPasteBuilder'),
        props: { fields: jsonFields.slice(0, 2), startParsed: true },
      },
      { id: 'xml-builder', component: component('XmlResponseBuilder'), props: { fields: xmlFields, rootTag: 'k0' } },
      {
        id: 'xml-paste',
        component: component('XmlPasteBuilder'),
        props: { fields: xmlFields, rootTag: 'k0', rootAttributes: [], startParsed: true },
      },
      {
        id: 'xml-paste-nested',
        component: component('XmlPasteBuilder'),
        props: { fields: xmlFields, rootTag: 'k0', rootAttributes: [], startParsed: true },
        open: (c) => click(c, 'xml-paste-builder-navigate-button-1'),
      },
    ],
  },
  {
    name: 'the errors of the by-example builders',
    screens: [
      {
        id: 'json-paste-error',
        component: component('JsonPasteBuilder'),
        open: (c) => pasteInvalidSample(c, 'json', '{oops'),
      },
      {
        id: 'xml-paste-error',
        component: component('XmlPasteBuilder'),
        open: (c) => pasteInvalidSample(c, 'xml', '<oops'),
      },
    ],
  },
  {
    name: 'the logs, groups, backups, TCP and sign-in screens',
    screens: [
      {
        id: 'request-log',
        component: component('RequestLog'),
        open: async (c) => {
          await appeared(c, 'request-log-detail-button-0');
          await click(c, 'request-log-detail-button-0');
        },
      },
      {
        id: 'messaging-log',
        component: component('MessagingLog'),
        open: async (c) => {
          await appeared(c, 'messaging-log-detail-button-0');
          await click(c, 'messaging-log-detail-button-0');
        },
      },
      {
        id: 'group-manager',
        component: component('GroupManager'),
        props: { services: [proxied, mocked], authEnabled: true },
        open: async (c) => {
          await appeared(c, 'group-manager-manage-button-g1');
          await click(c, 'group-manager-manage-button-g1');
          await click(c, 'group-manager-new-group-button');
        },
      },
      {
        id: 'backup-manager',
        component: component('BackupManager'),
        open: (c) => appeared(c, 'backup-manager-item-f1.yaml'),
      },
      { id: 'tcp-manager', component: component('TcpServiceManager'), open: (c) => appeared(c, 'tcp-manager-item-t1') },
      {
        id: 'tcp-manager-new',
        component: component('TcpServiceManager'),
        open: async (c) => {
          await appeared(c, 'tcp-manager-add-button');
          await click(c, 'tcp-manager-add-button');
        },
      },
      { id: 'login-form', component: component('LoginForm') },
    ],
  },
];
