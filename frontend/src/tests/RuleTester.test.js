import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RuleTester from '../lib/components/RuleTester.svelte';
import { testRule } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  testRule: vi.fn(),
}));

const logWithDetail = {
  timestamp: new Date('2026-01-10T12:00:00').getTime(),
  service_name: 'svc-a',
  method: 'GET',
  path: '/svc-a/orders/42',
  mode: 'mock',
  rule_matched: 'r1',
  target_url: null,
  status: 200,
  captured: {
    remaining_path: '/orders/42',
    path_params: {},
    query_params: { foo: 'bar' },
    headers: { 'x-env': 'prod' },
    body: '',
    body_truncated: false,
    content_type: null,
  },
};

const logWithoutDetail = {
  timestamp: new Date('2026-01-10T12:05:00').getTime(),
  service_name: 'svc-a',
  method: 'GET',
  path: '/svc-a/passthrough',
  mode: 'proxy',
  rule_matched: null,
  target_url: 'http://backend/passthrough',
  status: 200,
  captured: null,
};

function draft(overrides = {}) {
  return () => ({
    method: 'GET',
    subPath: '',
    allOf: [],
    anyOf: [],
    ...overrides,
  });
}

describe('RuleTester: entries without details are left out', () => {
  it('shows a message when no request has been captured', () => {
    const { getByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [], getDraftRule: draft() },
    });
    expect(getByText('No request has been captured for this service yet.')).toBeInTheDocument();
  });

  it('shows a message when only entries without details exist', () => {
    const { getByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithoutDetail], getDraftRule: draft() },
    });
    expect(getByText(/requests proxied directly/)).toBeInTheDocument();
  });

  it('lists only the entries with captured details in the picker', () => {
    const { getByLabelText, queryByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail, logWithoutDetail], getDraftRule: draft() },
    });
    const select = getByLabelText('Captured request');
    const options = [...select.querySelectorAll('option')].filter((o) => o.value !== '');
    expect(options).toHaveLength(1);
    expect(queryByText(/passthrough/)).not.toBeInTheDocument();
  });
});

describe('RuleTester: API call and result display', () => {
  it('sends the draft rule and the chosen captured request', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
    });

    const { getByLabelText, getByRole } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({
          allOf: [{ source: { type: 'QueryParam', key: 'foo' }, operator: { type: 'Eq', value: 'bar' } }],
        }),
      },
    });

    const select = getByLabelText('Captured request');
    await fireEvent.change(select, { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(testRule).toHaveBeenCalled());
    const [payload] = testRule.mock.calls.at(-1);
    expect(payload.method).toBe('GET');
    expect(payload.conditions.all_of[0].source.key).toBe('foo');
    expect(payload.request.query_params).toEqual({ foo: 'bar' });
    expect(payload.request.remaining_path).toBe('/orders/42');
  });

  it('sends the action and the 3 script slots of the draft, not only the matching', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
    });

    const { getByLabelText, getByRole } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({
          action: 'mock',
          preScript: 'let x = 1;',
          script: '"hello"',
          postScript: null,
        }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(testRule).toHaveBeenCalled());
    // `.at(-1)`, not `mock.calls[0]`: nothing resets `testRule` between the tests of this file, so `[0]` is the call
    // of an earlier test.
    const [payload] = testRule.mock.calls.at(-1);
    expect(payload.action).toBe('mock');
    expect(payload.pre_script).toBe('let x = 1;');
    expect(payload.script).toBe('"hello"');
    expect(payload.post_script).toBeNull();
  });

  it('shows the result of each condition in words, not only by color, with its hint', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: false,
      body_truncated: false,
      all_of: [
        {
          condition: { source: { type: 'QueryParam', key: 'foo' }, operator: { type: 'Eq', value: 'bar' } },
          matched: false,
          found_value: null,
          hint: "'foo' was not found as a query parameter, but it is present as a path parameter in this request",
        },
      ],
      any_of: [],
    });

    const { getByLabelText, getByRole, getByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail], getDraftRule: draft() },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByText('✗ This rule would not match this request')).toBeInTheDocument());
    expect(getByText(/: does not match \(/)).toBeInTheDocument();
    expect(getByText(/\(value found: none\)/)).toBeInTheDocument();
    expect(getByText(/present as a path parameter/)).toBeInTheDocument();
  });

  it('shows a warning when the captured body is truncated', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: false,
      body_truncated: true,
      all_of: [
        {
          condition: { source: { type: 'JsonPointer', key: '/a' }, operator: { type: 'Exists' } },
          matched: false,
          found_value: null,
          hint: null,
        },
      ],
      any_of: [],
    });

    const { getByLabelText, getByRole, getByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail], getDraftRule: draft() },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() =>
      expect(
        getByText('⚠ The body of this request was truncated in the log: comparisons on the body may be wrong.'),
      ).toBeInTheDocument(),
    );
  });

  it('shows a clear message when a script fails to run', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [
        { slot: 'script', message: 'Function not found: totally_undefined_fn (i64, i64) (line 1, position 1)' },
      ],
    });

    const { getByLabelText, getByRole, getByTestId, getByText } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: 'totally_undefined_fn(1, 2)' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByTestId('rule-tester-script-errors')).toBeInTheDocument());
    expect(getByText(/^⚠ A script failed to run:/)).toBeInTheDocument();
    expect(getByTestId('rule-tester-script-error-script')).toBeInTheDocument();
    expect(getByText(/totally_undefined_fn/)).toBeInTheDocument();
  });

  it('shows no script error when every script succeeds', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
    });

    const { getByLabelText, getByRole, queryByTestId } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: '"hello"' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(queryByTestId('rule-tester-result')).toBeInTheDocument());
    expect(queryByTestId('rule-tester-script-errors')).not.toBeInTheDocument();
  });

  // `script_results`: a script that runs without error can still produce the wrong result (a misspelled key, a nested
  // object read through a path), so the tester shows what the script produced, not only the absence of an error.

  it('shows the fields a successful script produces (e.g. an object picked by seeded_pick)', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
      script_results: [{ slot: 'script', value: '', fields: { name: 'Lyon', cp: '69000', insee: '69123' } }],
    });

    const { getByLabelText, getByRole, getByTestId, getByText } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: 'seeded_pick(request.path.siret, villes)' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByTestId('rule-tester-script-results')).toBeInTheDocument());
    expect(getByTestId('rule-tester-script-result-script')).toBeInTheDocument();
    expect(getByText('{{script.name}}')).toBeInTheDocument();
    expect(getByText('Lyon')).toBeInTheDocument();
    expect(getByText('{{script.cp}}')).toBeInTheDocument();
    expect(getByText('69000')).toBeInTheDocument();
  });

  it('shows a nested object as one flat field, with the JSON text the server sends', async () => {
    // An object nested under one key stays one flat field for `{{script.<key>}}`: the tester shows it as such, so the
    // user sees that a nested path such as `{{script.ville.name}}` does not exist.
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
      script_results: [
        {
          slot: 'script',
          value: '',
          fields: { ville: '{"cp":"69000","insee":"69123","name":"Lyon"}', id: 'fixed-id' },
        },
      ],
    });

    const { getByLabelText, getByRole, getByTestId, getByText } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: 'let ville = seeded_pick(...); #{ ville: ville, id: "fixed-id" }' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByTestId('rule-tester-script-results')).toBeInTheDocument());
    expect(getByText('{{script.ville}}')).toBeInTheDocument();
    expect(getByText('{"cp":"69000","insee":"69123","name":"Lyon"}')).toBeInTheDocument();
  });

  it('shows the plain value ({{slot}}) when the script returns a scalar, not a map', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
      script_results: [{ slot: 'script', value: 'hello', fields: {} }],
    });

    const { getByLabelText, getByRole, getByTestId, getByText } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: '"hello"' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByTestId('rule-tester-script-results')).toBeInTheDocument());
    expect(getByText('{{script}}')).toBeInTheDocument();
    expect(getByText('hello')).toBeInTheDocument();
  });

  it('shows no script result panel when no script is configured', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
      script_results: [],
    });

    const { getByLabelText, getByRole, queryByTestId } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail], getDraftRule: draft() },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(queryByTestId('rule-tester-result')).toBeInTheDocument());
    expect(queryByTestId('rule-tester-script-results')).not.toBeInTheDocument();
  });
});

describe('RuleTester: script errors', () => {
  async function scriptErrorText() {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [{ slot: 'script', message: 'boom' }],
    });
    const { container, getByTestId } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail], getDraftRule: draft({ script: 'boom()' }) },
    });
    await fireEvent.change(container.querySelector('[data-testid="rule-tester-log-select"]'), {
      target: { value: '0' },
    });
    await fireEvent.click(container.querySelector('[data-testid="rule-tester-test-button"]'));
    await waitFor(() => expect(getByTestId('rule-tester-script-error-script')).toBeInTheDocument());
    return getByTestId('rule-tester-script-error-script').textContent.trim();
  }

  it('punctuates the slot and its error as English does', async () => {
    expect(await scriptErrorText()).toBe('Custom script: boom');
  });
});
