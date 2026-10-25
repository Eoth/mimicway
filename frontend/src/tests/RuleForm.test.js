import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RuleForm from '../lib/components/RuleForm.svelte';
import { getLogs, checkRuleConflicts } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  validateScript: vi.fn(),
  getLogs: vi.fn(),
  checkRuleConflicts: vi.fn(),
}));

async function setInput(el, value) {
  el.value = value;
  await fireEvent.input(el);
}

async function submitForm(container) {
  const form = container.querySelector('form');
  await fireEvent.submit(form);
}

describe('RuleForm: rule name uniqueness', () => {
  it('refuses a rule name that already exists in the service', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(RuleForm, {
      props: { existingRules: [{ name: 'existing-rule' }], onSave },
    });

    await setInput(getByLabelText('Rule name'), 'existing-rule');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('A rule named "existing-rule" already exists in this service.');
  });

  it('refuses a duplicate that differs only in case', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(RuleForm, {
      props: { existingRules: [{ name: 'My-Rule' }], onSave },
    });

    await setInput(getByLabelText('Rule name'), 'my-rule');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('A rule named "my-rule" already exists in this service.');
  });

  it('accepts a unique name', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, container } = render(RuleForm, {
      props: { existingRules: [{ name: 'other-rule' }], onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalled());
  });

  it('accepts its own name when editing, the caller leaving it out of `existingRules`', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const existingRule = {
      name: 'edit-me',
      action: 'mock',
      conditions: { all_of: [], any_of: [] },
      response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'ok' }], chaos: null },
    };
    const { container } = render(RuleForm, {
      props: { rule: existingRule, existingRules: [], onSave },
    });

    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalled());
  });

  it('refuses an empty rule name', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(RuleForm, {
      props: { onSave },
    });

    await setInput(getByLabelText('Rule name'), '');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('The rule name is required.');
  });
});

describe('RuleForm: one script per rule', () => {
  it('shows a single script toggle and no folded script blocks', () => {
    const { getAllByRole, queryByRole } = render(RuleForm);
    expect(getAllByRole('switch', { name: /script/i })).toHaveLength(1);
    expect(queryByRole('button', { name: /Advanced options/ })).not.toBeInTheDocument();
  });

  it('sends the script and none of the removed blocks', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, getByRole, container } = render(RuleForm, { props: { onSave } });

    await setInput(getByLabelText('Rule name'), 'r1');
    await fireEvent.click(getByRole('switch', { name: 'Custom script' }));
    await setInput(container.querySelector('#rule-script'), '"only"');
    await submitForm(container);

    await waitFor(() => expect(onSave).toHaveBeenCalled());
    const [payload] = onSave.mock.calls[0];
    expect(payload.script).toBe('"only"');
    expect(payload).not.toHaveProperty('pre_script');
    expect(payload).not.toHaveProperty('post_script');
  });
});

describe('RuleForm: input help for path and query params', () => {
  it('does not render the rule tester without `serviceName`', () => {
    const { queryByText } = render(RuleForm);
    expect(queryByText('Test against a real request')).not.toBeInTheDocument();
  });

  it('renders the rule tester when `serviceName` is given', async () => {
    getLogs.mockResolvedValue([]);
    const { getByText } = render(RuleForm, { props: { serviceName: 'svc-a' } });
    await waitFor(() => expect(getByText('Test against a real request')).toBeInTheDocument());
  });

  it('offers the path params of the service path and of the sub-path together', async () => {
    getLogs.mockResolvedValue([]);
    const { getByLabelText, getByRole, getByText } = render(RuleForm, {
      props: { serviceName: 'svc-a', listenPath: '/orders/{id}' },
    });
    await setInput(getByLabelText('Sub-path (optional)'), '/items/{itemId}');
    await fireEvent.click(getByRole('button', { name: '+ AND condition' }));
    const sourceSelect = getByLabelText('Source');
    expect(sourceSelect.querySelector('option[value="PathParam"]')).toBeInTheDocument();
    expect(getByText('2 path parameters available: id, itemId')).toBeInTheDocument();
  });
});

describe('RuleForm: conflict check on save', () => {
  const existingRules = [
    {
      name: 'existing-rule',
      method: 'GET',
      sub_path: null,
      conditions: { all_of: [], any_of: [] },
    },
  ];

  it('saves at once, without a warning, when no conflict is found', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, container, queryByTestId } = render(RuleForm, {
      props: { existingRules, draftPosition: 1, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(queryByTestId('rule-form-conflict-warning')).not.toBeInTheDocument();
  });

  it('shows a warning and does not call onSave when a conflict is found', async () => {
    checkRuleConflicts.mockResolvedValue({
      conflicts: [{ other_rule_name: 'existing-rule', winner: 'other' }],
    });
    const onSave = vi.fn();
    const { getByLabelText, container, findByTestId, getByRole } = render(RuleForm, {
      props: { existingRules, draftPosition: 1, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);

    const warning = await findByTestId('rule-form-conflict-warning');
    expect(warning).toHaveTextContent('existing-rule');
    expect(getByRole('alert')).toBe(warning);
    expect(onSave).not.toHaveBeenCalled();
  });

  it('saves anyway despite the warning', async () => {
    checkRuleConflicts.mockResolvedValue({
      conflicts: [{ other_rule_name: 'existing-rule', winner: 'draft' }],
    });
    const onSave = vi.fn();
    const { getByLabelText, container, findByTestId, getByTestId } = render(RuleForm, {
      props: { existingRules, draftPosition: 0, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);
    await findByTestId('rule-form-conflict-warning');

    await fireEvent.click(getByTestId('rule-form-conflict-save-anyway-button'));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0][0].name).toBe('new-rule');
  });

  it('cancelling the warning closes it without saving', async () => {
    checkRuleConflicts.mockResolvedValue({
      conflicts: [{ other_rule_name: 'existing-rule', winner: 'other' }],
    });
    const onSave = vi.fn();
    const { getByLabelText, container, findByTestId, getByTestId, queryByTestId } = render(RuleForm, {
      props: { existingRules, draftPosition: 1, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);
    await findByTestId('rule-form-conflict-warning');

    await fireEvent.click(getByTestId('rule-form-conflict-cancel-button'));
    expect(queryByTestId('rule-form-conflict-warning')).not.toBeInTheDocument();
    expect(onSave).not.toHaveBeenCalled();
  });

  it('saves anyway when the conflict check fails (fail open)', async () => {
    checkRuleConflicts.mockRejectedValue(new Error('network unreachable'));
    const onSave = vi.fn();
    const { getByLabelText, container } = render(RuleForm, {
      props: { existingRules, draftPosition: 1, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  });
});

describe('RuleForm: no proxy action for a purely mocked service', () => {
  it('shows both actions, Mock and Proxy, when the service has a target', () => {
    const { getByTestId, queryByTestId } = render(RuleForm, {
      props: { isPurelyMocked: false },
    });

    expect(getByTestId('rule-form-action-mock-radio')).toBeInTheDocument();
    expect(getByTestId('rule-form-action-proxy-radio')).toBeInTheDocument();
    expect(queryByTestId('rule-form-purely-mocked-hint')).not.toBeInTheDocument();
  });

  it('hides the Proxy action when the service is purely mocked', () => {
    const { getByTestId, queryByTestId } = render(RuleForm, {
      props: { isPurelyMocked: true },
    });

    expect(getByTestId('rule-form-action-mock-radio')).toBeInTheDocument();
    expect(queryByTestId('rule-form-action-proxy-radio')).not.toBeInTheDocument();
  });

  it('a proxy rule opened on a purely mocked service is saved as a mock', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const staleRule = {
      name: 'stale-proxy-rule',
      action: 'proxy',
      conditions: { all_of: [], any_of: [] },
      response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'ok' }], chaos: null },
    };
    const { container, getByTestId } = render(RuleForm, {
      props: { rule: staleRule, existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);
    // The warning about the change of action comes first: the submit alone does not save.
    expect(onSave).not.toHaveBeenCalled();
    await fireEvent.click(getByTestId('rule-form-stale-proxy-save-anyway-button'));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].action).toBe('mock');
  });
});

describe('RuleForm: warning before saving the change from proxy to mock', () => {
  function staleProxyRule(overrides = {}) {
    return {
      name: 'stale-proxy-rule',
      action: 'proxy',
      conditions: { all_of: [], any_of: [] },
      response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'ok' }], chaos: null },
      ...overrides,
    };
  }

  it('shows a warning naming both actions and does not save until it is confirmed', async () => {
    const onSave = vi.fn();
    // The `checkRuleConflicts` mock is shared by every test of this file and never reset: the test compares the call
    // count before and after rather than expect zero.
    const conflictCallsBefore = checkRuleConflicts.mock.calls.length;
    const { container, getByTestId } = render(RuleForm, {
      props: { rule: staleProxyRule(), existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);

    const warning = getByTestId('rule-form-stale-proxy-warning');
    expect(warning).toHaveTextContent('Proxy');
    expect(warning).toHaveTextContent('Mock');
    expect(onSave).not.toHaveBeenCalled();
    // The conflict check, a server call, waits until the local warning is resolved.
    expect(checkRuleConflicts.mock.calls.length).toBe(conflictCallsBefore);
  });

  it('"Save anyway" saves the change to mock', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { container, getByTestId, queryByTestId } = render(RuleForm, {
      props: { rule: staleProxyRule(), existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);
    await fireEvent.click(getByTestId('rule-form-stale-proxy-save-anyway-button'));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].action).toBe('mock');
    expect(queryByTestId('rule-form-stale-proxy-warning')).not.toBeInTheDocument();
  });

  it('"Edit the rule" closes the warning without saving', async () => {
    const onSave = vi.fn();
    const { container, getByTestId, queryByTestId } = render(RuleForm, {
      props: { rule: staleProxyRule(), existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);
    await fireEvent.click(getByTestId('rule-form-stale-proxy-cancel-button'));

    expect(onSave).not.toHaveBeenCalled();
    expect(queryByTestId('rule-form-stale-proxy-warning')).not.toBeInTheDocument();
  });

  it('never shows for an ordinary mock rule on a purely mocked service', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const ordinaryRule = staleProxyRule({ name: 'ordinary-mock-rule', action: 'mock' });
    const { container, queryByTestId } = render(RuleForm, {
      props: { rule: ordinaryRule, existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(queryByTestId('rule-form-stale-proxy-warning')).not.toBeInTheDocument();
  });

  it('never shows for a proxy rule on a service that has a target', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { container, queryByTestId } = render(RuleForm, {
      props: { rule: staleProxyRule(), existingRules: [], isPurelyMocked: false, onSave },
    });

    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].action).toBe('proxy');
    expect(queryByTestId('rule-form-stale-proxy-warning')).not.toBeInTheDocument();
  });
});

// The structured views all save a single Template fragment, which does not tell them apart: `Rule.response_mode`
// records the view, so that reopening a rule shows that view rather than the advanced template.
describe('RuleForm: reopening a rule shows the view that built it', () => {
  it('a rule saved in the JSON by-example view reopens in that view', () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      response_mode: 'json-paste',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [{ name: 'Content-Type', value: 'application/json' }],
        body: [{ type: 'Template', template: '{"siret":"{{path.siret}}"}' }],
        chaos: null,
      },
    };
    const { container, queryByTestId } = render(RuleForm, { props: { rule } });

    // The view shows the field read back from the template: neither the paste area nor the key input of the detailed
    // view (json-guided).
    const sourceSelect = container.querySelector('[data-testid="json-paste-builder-source-select-0"]');
    expect(sourceSelect).toBeInTheDocument();
    expect(sourceSelect.value).toBe('path');
    expect(queryByTestId('json-paste-builder-textarea')).not.toBeInTheDocument();
    expect(queryByTestId('json-builder-key-input-0')).not.toBeInTheDocument();
  });

  it('a rule saved in the detailed JSON view reopens in that view', () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      response_mode: 'json-guided',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [{ name: 'Content-Type', value: 'application/json' }],
        body: [{ type: 'Template', template: '{"siret":"{{path.siret}}"}' }],
        chaos: null,
      },
    };
    const { queryByTestId } = render(RuleForm, { props: { rule } });

    const keyInput = queryByTestId('json-builder-key-input-0');
    expect(keyInput).toBeInTheDocument();
    expect(keyInput.value).toBe('siret');
    // The "Edit in detail" button belongs to the by-example view only.
    expect(queryByTestId('rule-form-open-detail-button')).not.toBeInTheDocument();
  });

  it('a rule saved in the XML by-example view reopens in that view, with its pipe', () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      response_mode: 'xml-paste',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [{ name: 'Content-Type', value: 'application/xml' }],
        body: [{ type: 'Template', template: '<response><siret>{{path.siret | upper}}</siret></response>' }],
        chaos: null,
      },
    };
    const { container, queryByTestId } = render(RuleForm, { props: { rule } });

    const sourceSelect = container.querySelector('[data-testid="xml-paste-builder-source-select-0"]');
    expect(sourceSelect).toBeInTheDocument();
    expect(sourceSelect.value).toBe('path');
    const pipeInput = container.querySelector('[data-testid="xml-paste-builder-pipe-input-0"]');
    expect(pipeInput.value).toBe('upper');
    expect(queryByTestId('xml-paste-builder-textarea')).not.toBeInTheDocument();
  });

  it('a rule saved without `response_mode` falls back to the advanced template for a single Template fragment', () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [{ name: 'Content-Type', value: 'application/json' }],
        body: [{ type: 'Template', template: '{"siret":"{{path.siret}}"}' }],
        chaos: null,
      },
    };
    const { queryByTestId } = render(RuleForm, { props: { rule } });

    expect(queryByTestId('rule-form-fragment-template-textarea-0')).toBeInTheDocument();
    expect(queryByTestId('json-paste-builder-source-select-0')).not.toBeInTheDocument();
  });

  it('a structured `response_mode` with a body of another shape opens the advanced template instead of failing', () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      response_mode: 'json-paste',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [],
        body: [{ type: 'Literal', value: 'plain text' }],
        chaos: null,
      },
    };
    const { queryByTestId } = render(RuleForm, { props: { rule } });
    expect(queryByTestId('rule-form-fragment-literal-textarea-0')).toBeInTheDocument();
  });
});

// One button per format: JSON and XML each have two views, by example and in detail, and the "Edit in detail" button
// switches between them, so the format selector stays a single row.
describe('RuleForm: one button per format, two views for JSON and XML', () => {
  it('shows six format buttons: JSON, XML, Text, Advanced template, Event stream and Empty', () => {
    const { container } = render(RuleForm);
    const buttons = container.querySelectorAll('[data-testid^="rule-form-mode-button-"]');
    expect(buttons).toHaveLength(6);
    const testids = [...buttons].map((b) => b.dataset.testid);
    expect(testids).toEqual([
      'rule-form-mode-button-json',
      'rule-form-mode-button-xml',
      'rule-form-mode-button-text',
      'rule-form-mode-button-advanced',
      'rule-form-mode-button-events',
      'rule-form-mode-button-empty',
    ]);
  });

  it('a new rule in JSON shows the by-example view first, with the "Edit in detail" button', async () => {
    const { container, queryByTestId } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));

    expect(queryByTestId('json-paste-builder-textarea')).toBeInTheDocument();
    expect(queryByTestId('rule-form-open-detail-button')).toBeInTheDocument();
  });

  it('"Edit in detail" shows the detailed view without a data loss warning', async () => {
    const { container, queryByTestId, queryByRole } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));

    expect(queryByTestId('json-builder-add-field-button')).toBeInTheDocument();
    expect(queryByRole('alert')).not.toBeInTheDocument();
  });

  it('clicking the active format button again keeps the structure being built', async () => {
    const { container } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="json-builder-add-field-button"]'));
    const keyInput = container.querySelector('[data-testid="json-builder-key-input-0"]');
    await setInput(keyInput, 'total');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));

    expect(container.querySelector('[data-testid="json-builder-key-input-0"]').value).toBe('total');
  });
});

// A well-formed XML advanced template converts to the XML format without a warning, as a JSON one converts to JSON.
describe('RuleForm: converting an advanced template to XML', () => {
  async function buildAdvancedXmlTemplate(container, getByLabelText, tpl) {
    await setInput(getByLabelText('Rule name'), 'test-rule');
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-advanced"]'));
    const typeSelect = container.querySelector('[data-testid="rule-form-fragment-type-select-0"]');
    await fireEvent.change(typeSelect, { target: { value: 'Template' } });
    const tplTextarea = container.querySelector('[data-testid="rule-form-fragment-template-textarea-0"]');
    await setInput(tplTextarea, tpl);
  }

  it('a valid XML advanced template converts to the XML by-example view without a warning', async () => {
    const { container, getByLabelText, queryByRole } = render(RuleForm);
    await buildAdvancedXmlTemplate(container, getByLabelText, '<response><nom>ACME</nom></response>');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-xml"]'));

    expect(queryByRole('alert')).not.toBeInTheDocument();
    const sourceSelect = container.querySelector('[data-testid="xml-paste-builder-source-select-0"]');
    expect(sourceSelect).toBeInTheDocument();
    expect(container.querySelector('[data-testid="xml-paste-builder-value-input-0"]').value).toBe('ACME');
  });

  it('the conversion keeps the root attributes of the advanced template', async () => {
    const { container, getByLabelText } = render(RuleForm);
    await buildAdvancedXmlTemplate(container, getByLabelText, '<devisResponse ver="1"><nom>ACME</nom></devisResponse>');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-xml"]'));

    const row = container
      .querySelector('[data-testid^="xml-paste-builder-attr-source-select-root"]')
      .closest('.paste-attr-row');
    expect(row.querySelector('.paste-attr-name')).toHaveTextContent('@ver');
    expect(row.querySelector('.paste-preview-fixed')).toHaveTextContent('1');
  });

  it('an invalid XML advanced template gets a conversion warning', async () => {
    const { container, getByLabelText, queryByRole } = render(RuleForm);
    await buildAdvancedXmlTemplate(container, getByLabelText, '<response><nom>ACME</response>');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-xml"]'));

    expect(queryByRole('alert')).toHaveTextContent('Cannot convert:');
  });
});

// `backToPasteMode()` mirrors `revealDetailMode()`: both views share the shape of their fields, so going back copies
// them without a data loss warning.
describe('RuleForm: back to the by-example view from the detailed one', () => {
  it('the back button is absent from the by-example view and present in the detailed one', async () => {
    const { container, queryByTestId } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    expect(queryByTestId('rule-form-back-to-paste-button')).not.toBeInTheDocument();

    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));
    expect(queryByTestId('rule-form-back-to-paste-button')).toBeInTheDocument();
  });

  it('JSON: going back to the by-example view keeps the content, without a warning', async () => {
    const { container, queryByRole } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    const textarea = container.querySelector('[data-testid="json-paste-builder-textarea"]');
    await setInput(textarea, '{"nom":"ACME"}');
    await fireEvent.click(container.querySelector('[data-testid="json-paste-builder-analyze-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));

    await fireEvent.click(container.querySelector('[data-testid="rule-form-back-to-paste-button"]'));

    expect(queryByRole('alert')).not.toBeInTheDocument();
    // The fields show at once, not an empty paste area: `startParsed` is read when the builder mounts again, not
    // fixed when the form opened.
    expect(container.querySelector('[data-testid="json-paste-builder-textarea"]')).not.toBeInTheDocument();
    const sourceSelect = container.querySelector('[data-testid="json-paste-builder-source-select-0"]');
    expect(sourceSelect).toBeInTheDocument();
  });

  it('XML: going back to the by-example view keeps the content, without a warning', async () => {
    const { container, queryByRole } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-xml"]'));
    const textarea = container.querySelector('[data-testid="xml-paste-builder-textarea"]');
    await setInput(textarea, '<response><nom>ACME</nom></response>');
    await fireEvent.click(container.querySelector('[data-testid="xml-paste-builder-analyze-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));

    await fireEvent.click(container.querySelector('[data-testid="rule-form-back-to-paste-button"]'));

    expect(queryByRole('alert')).not.toBeInTheDocument();
    expect(container.querySelector('[data-testid="xml-paste-builder-textarea"]')).not.toBeInTheDocument();
    expect(container.querySelector('[data-testid="xml-paste-builder-source-select-0"]')).toBeInTheDocument();
  });

  it('a round trip from the detailed view to the by-example one and back keeps the changes made in detail', async () => {
    const { container } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    const textarea = container.querySelector('[data-testid="json-paste-builder-textarea"]');
    await setInput(textarea, '{"nom":"ACME"}');
    await fireEvent.click(container.querySelector('[data-testid="json-paste-builder-analyze-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="json-builder-add-field-button"]'));
    await setInput(container.querySelector('[data-testid="json-builder-key-input-1"]'), 'siret');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-back-to-paste-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));

    expect(container.querySelector('[data-testid="json-builder-key-input-0"]').value).toBe('nom');
    expect(container.querySelector('[data-testid="json-builder-key-input-1"]').value).toBe('siret');
  });
});

// The whole round trip: paste a sample, give a field a source and a pipe, and find `{{expr | pipe}}` in the saved
// template.
describe('RuleForm: pipes in the by-example view', () => {
  it('a pipe set in the JSON by-example view reaches the saved template', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, container } = render(RuleForm, { props: { onSave } });

    await setInput(getByLabelText('Rule name'), 'pipe-rule');
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    const textarea = container.querySelector('[data-testid="json-paste-builder-textarea"]');
    await setInput(textarea, '{"siret":"00000000000000"}');
    await fireEvent.click(container.querySelector('[data-testid="json-paste-builder-analyze-button"]'));

    const sourceSelect = container.querySelector('[data-testid="json-paste-builder-source-select-0"]');
    await fireEvent.change(sourceSelect, { target: { value: 'path' } });
    await setInput(container.querySelector('[data-testid="json-paste-builder-value-input-0"]'), 'siret');
    await setInput(container.querySelector('[data-testid="json-paste-builder-pipe-input-0"]'), 'upper');

    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalled());
    const [payload] = onSave.mock.calls[0];
    expect(payload.response.body[0].template).toContain('{{path.siret | upper}}');
    expect(payload.response_mode).toBe('json-paste');
  });
});

// The response must not depend on which builder is mounted: folding the section, switching the action or the view
// unmounts the builders, and what they showed has to be saved all the same.
describe('RuleForm: the response survives the builders being unmounted', () => {
  const byId = (container, id) => container.querySelector(`[data-testid="${id}"]`);

  async function renderNamed() {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const view = render(RuleForm, { props: { onSave } });
    await setInput(byId(view.container, 'rule-form-name-input'), 'kept');
    return { ...view, onSave };
  }

  async function pasteJson(container, json) {
    await setInput(byId(container, 'json-paste-builder-textarea'), json);
    await fireEvent.click(byId(container, 'json-paste-builder-analyze-button'));
  }

  async function pasteXml(container, xml) {
    await fireEvent.click(byId(container, 'rule-form-mode-button-xml'));
    await setInput(byId(container, 'xml-paste-builder-textarea'), xml);
    await fireEvent.click(byId(container, 'xml-paste-builder-analyze-button'));
  }

  async function savedTemplate(container, onSave) {
    const calls = onSave.mock.calls.length;
    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(calls + 1));
    const [payload] = onSave.mock.calls.at(-1);
    return payload.response.body.map((f) => f.template ?? f.value).join('');
  }

  async function toggleResponseSection(container) {
    await fireEvent.click(byId(container, 'rule-form-response-toggle-button'));
  }

  it('saves the pasted JSON while the response section is folded', async () => {
    const { container, onSave } = await renderNamed();
    await pasteJson(container, '{"id":"42"}');
    await toggleResponseSection(container);

    expect(await savedTemplate(container, onSave)).toBe('{"id":"42"}');
  });

  it('saves the response edited before the rule was switched to proxy', async () => {
    const { container, onSave } = await renderNamed();
    await pasteJson(container, '{"id":"42"}');
    await fireEvent.change(byId(container, 'rule-form-action-proxy-radio'));

    expect(await savedTemplate(container, onSave)).toBe('{"id":"42"}');
  });

  it('keeps the root tag and attributes of pasted XML in the detailed view and back', async () => {
    const { container, onSave } = await renderNamed();
    await pasteXml(container, '<order id="7"><a>x</a></order>');
    await fireEvent.click(byId(container, 'rule-form-open-detail-button'));

    expect(byId(container, 'xml-builder-root-tag-input').value).toBe('order');
    expect(await savedTemplate(container, onSave)).toBe('<order id="7"><a>x</a></order>');

    await fireEvent.click(byId(container, 'rule-form-back-to-paste-button'));
    expect(await savedTemplate(container, onSave)).toBe('<order id="7"><a>x</a></order>');
  });

  it('keeps a root tag renamed in the detailed view when going back to the example', async () => {
    const { container, onSave } = await renderNamed();
    await pasteXml(container, '<order><a>x</a></order>');
    await fireEvent.click(byId(container, 'rule-form-open-detail-button'));
    await setInput(byId(container, 'xml-builder-root-tag-input'), 'reply');
    await fireEvent.click(byId(container, 'rule-form-back-to-paste-button'));

    expect(await savedTemplate(container, onSave)).toBe('<reply><a>x</a></reply>');
  });

  it('keeps the root tag of pasted XML when the section is folded and opened again', async () => {
    const { container, onSave } = await renderNamed();
    await pasteXml(container, '<order id="7"><a>x</a></order>');
    await toggleResponseSection(container);
    await toggleResponseSection(container);

    expect(await savedTemplate(container, onSave)).toBe('<order id="7"><a>x</a></order>');
  });

  it('keeps a JSON array pasted by example when the section is folded and opened again', async () => {
    const { container, onSave } = await renderNamed();
    await pasteJson(container, '[{"id":"1"}]');
    await toggleResponseSection(container);
    await toggleResponseSection(container);

    expect(await savedTemplate(container, onSave)).toBe('[{"id":"1"}]');
  });

  it('keeps a JSON array pasted by example in the detailed view, and says so', async () => {
    const { container, onSave } = await renderNamed();
    await pasteJson(container, '[{"id":"1"}]');
    await fireEvent.click(byId(container, 'rule-form-open-detail-button'));

    expect(byId(container, 'json-builder-array-root-hint')).toBeInTheDocument();
    expect(await savedTemplate(container, onSave)).toBe('[{"id":"1"}]');
  });

  it('reopens a rule whose JSON body by example is an array in that view, and saves it unchanged', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const rule = {
      name: 'list',
      method: 'GET',
      action: 'mock',
      response_mode: 'json-paste',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [],
        body: [{ type: 'Template', template: '[{"id":"{{path.id}}"}]' }],
        chaos: null,
      },
    };
    const { container } = render(RuleForm, { props: { rule, onSave } });

    expect(byId(container, 'json-paste-builder-source-select-0')).toBeInTheDocument();
    expect(await savedTemplate(container, onSave)).toBe('[{"id":"{{path.id}}"}]');
  });
});

describe('RuleForm: the traffic of its own service', () => {
  const captured = {
    remaining_path: '/1',
    path_params: {},
    query_params: {},
    headers: {},
    body: '',
    body_truncated: false,
    content_type: null,
  };
  const log = (group_name, path) => ({
    timestamp: 1,
    service_name: 'users',
    group_name,
    method: 'GET',
    path,
    mode: 'mock',
    captured,
  });

  it('offers the rule tester the requests of its service only, not those of a namesake in another group', async () => {
    getLogs.mockResolvedValue([log('team-a', '/a/users/1'), log('team-b', '/b/users/2'), log(null, '/users/3')]);
    const { container } = render(RuleForm, { props: { serviceName: 'users', groupName: 'team-a' } });

    await waitFor(() => expect(container.querySelector('[data-testid="rule-tester-log-select"]')).not.toBeNull());
    const options = [...container.querySelectorAll('[data-testid="rule-tester-log-select"] option:not([disabled])')];
    expect(options.map((o) => o.textContent)).toEqual([expect.stringContaining('/a/users/1')]);
  });
});

describe('RuleForm: converting a by-example response to the advanced template', () => {
  const byId = (container, id) => container.querySelector(`[data-testid="${id}"]`);

  async function paste(container, format, sample) {
    await fireEvent.click(byId(container, `rule-form-mode-button-${format}`));
    await setInput(byId(container, `${format}-paste-builder-textarea`), sample);
    await fireEvent.click(byId(container, `${format}-paste-builder-analyze-button`));
  }

  it('turns a JSON pasted by example into its template, without a warning', async () => {
    const { container } = render(RuleForm);
    await paste(container, 'json', '{"id":"42"}');
    await fireEvent.click(byId(container, 'rule-form-mode-button-advanced'));

    expect(byId(container, 'rule-form-mode-switch-confirm-button')).toBeNull();
    expect(byId(container, 'rule-form-fragment-template-textarea-0').value).toBe('{"id":"42"}');
  });

  it('turns an XML pasted by example into its template, without a warning', async () => {
    const { container } = render(RuleForm);
    await paste(container, 'xml', '<order id="7"><a>x</a></order>');
    await fireEvent.click(byId(container, 'rule-form-mode-button-advanced'));

    expect(byId(container, 'rule-form-mode-switch-confirm-button')).toBeNull();
    expect(byId(container, 'rule-form-fragment-template-textarea-0').value).toBe('<order id="7"><a>x</a></order>');
  });
});
