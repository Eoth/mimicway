import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RuleForm from '../lib/components/RuleForm.svelte';
import RuleEventsEditor from '../lib/components/RuleEventsEditor.svelte';
import { checkRuleConflicts } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  validateScript: vi.fn(),
  getLogs: vi.fn(),
  checkRuleConflicts: vi.fn(),
}));

const streamingRule = {
  name: 'log',
  method: 'GET',
  action: 'mock',
  conditions: { all_of: [], any_of: [] },
  response_mode: 'events',
  response: {
    status: 200,
    headers: [],
    body: [],
    chaos: null,
    events: {
      cut_at: 1,
      events: [
        { event: null, data: 'pulling image', delay_ms: 0 },
        { event: null, data: 'never sent', delay_ms: 50 },
        { event: 'endofstream', data: 'endofstream', delay_ms: 0 },
      ],
    },
  },
};

async function save(container, onSave) {
  checkRuleConflicts.mockResolvedValue({ conflicts: [] });
  await fireEvent.submit(container.querySelector('form'));
  await waitFor(() => expect(onSave).toHaveBeenCalled());
  return onSave.mock.calls.at(-1)[0];
}

describe('Rule form: a response sent as server-sent events', () => {
  it('reopens a rule with events on its events, and saves them back unchanged with no body', async () => {
    const onSave = vi.fn();
    const { container, getByTestId } = render(RuleForm, { props: { rule: streamingRule, existingRules: [], onSave } });

    expect(getByTestId('rule-form-mode-button-events')).toHaveAttribute('aria-checked', 'true');
    expect(getByTestId('rule-events-data-input-0')).toHaveValue('pulling image');
    expect(getByTestId('rule-events-name-input-2')).toHaveValue('endofstream');
    expect(getByTestId('rule-events-end-select')).toHaveValue('1');

    const saved = await save(container, onSave);
    expect(saved.response.events).toEqual(streamingRule.response.events);
    expect(saved.response.body).toEqual([]);
    expect(saved.response_mode).toBe('events');
  });

  it('builds a sequence from an empty rule: data, name, delay, then a cut', async () => {
    const onSave = vi.fn();
    const { container, getByLabelText, getByTestId } = render(RuleForm, { props: { existingRules: [], onSave } });
    await fireEvent.input(getByLabelText('Rule name'), { target: { value: 'feed' } });
    await fireEvent.click(getByTestId('rule-form-mode-button-events'));

    await fireEvent.input(getByTestId('rule-events-data-input-0'), { target: { value: 'line {{seq}}' } });
    await fireEvent.click(getByTestId('rule-events-add-button'));
    await fireEvent.input(getByTestId('rule-events-name-input-1'), { target: { value: 'end' } });
    await fireEvent.input(getByTestId('rule-events-delay-input-1'), { target: { value: '250' } });
    await fireEvent.input(getByTestId('rule-events-data-input-1'), { target: { value: 'done' } });
    await fireEvent.change(getByTestId('rule-events-end-select'), { target: { value: '1' } });

    const saved = await save(container, onSave);
    expect(saved.response.events).toEqual({
      cut_at: 1,
      events: [
        { event: null, data: 'line {{seq}}', delay_ms: 0 },
        { event: 'end', data: 'done', delay_ms: 250 },
      ],
    });
  });

  it('refuses to save a stream without any event', async () => {
    const onSave = vi.fn();
    const { container, getByLabelText, getByTestId, getByRole } = render(RuleForm, {
      props: { existingRules: [], onSave },
    });
    await fireEvent.input(getByLabelText('Rule name'), { target: { value: 'feed' } });
    await fireEvent.click(getByTestId('rule-form-mode-button-events'));
    await fireEvent.click(getByTestId('rule-events-remove-button-0'));
    await fireEvent.submit(container.querySelector('form'));
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('An event stream needs at least one event.');
  });

  it('saves no events once another format is chosen', async () => {
    const onSave = vi.fn();
    const { container, getByTestId } = render(RuleForm, { props: { rule: streamingRule, existingRules: [], onSave } });
    // Events cannot become a text: the form warns before switching.
    await fireEvent.click(getByTestId('rule-form-mode-button-text'));
    await fireEvent.click(getByTestId('rule-form-mode-switch-confirm-button'));
    const saved = await save(container, onSave);
    expect(saved.response.events).toBeNull();
    expect(saved.response.body).toEqual([{ type: 'Literal', value: '' }]);
  });
});

describe('RuleEventsEditor: the cut follows its event', () => {
  const three = [
    { event: null, data: 'a', delay_ms: 0 },
    { event: null, data: 'b', delay_ms: 0 },
    { event: null, data: 'c', delay_ms: 0 },
  ];

  it('moves up when an event before it is removed, and goes with the event it replaces', async () => {
    const onUpdate = vi.fn();
    const onCutChange = vi.fn();
    const { getByTestId, rerender } = render(RuleEventsEditor, {
      props: { events: three, cutAt: 2, onUpdate, onCutChange },
    });
    await fireEvent.click(getByTestId('rule-events-remove-button-0'));
    expect(onUpdate).toHaveBeenLastCalledWith([three[1], three[2]]);
    expect(onCutChange).toHaveBeenLastCalledWith(1);

    await rerender({ events: three, cutAt: 1, onUpdate, onCutChange });
    await fireEvent.click(getByTestId('rule-events-remove-button-1'));
    expect(onCutChange).toHaveBeenLastCalledWith(null);
  });

  it('reads a negative or empty delay as no delay', async () => {
    const onUpdate = vi.fn();
    const { getByTestId } = render(RuleEventsEditor, { props: { events: three, onUpdate } });
    await fireEvent.input(getByTestId('rule-events-delay-input-0'), { target: { value: '-5' } });
    expect(onUpdate.mock.calls.at(-1)[0][0].delay_ms).toBe(0);
  });
});
