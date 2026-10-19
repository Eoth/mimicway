import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RuleConditionsEditor from '../lib/components/RuleConditionsEditor.svelte';
import RuleConditionsEditorHarness from './helpers/RuleConditionsEditorHarness.svelte';

async function setInput(el, value) {
  el.value = value;
  await fireEvent.input(el);
}

describe('RuleConditionsEditor: in-place edit of an existing condition', () => {
  const allOf = [
    { source: { type: 'QueryParam', key: 'debug' }, operator: { type: 'Eq', value: '1' } },
    { source: { type: 'Header', key: 'X-Trace' }, operator: { type: 'Exists' } },
  ];

  it('shows each condition as a button, not plain text', () => {
    const { getByTestId } = render(RuleConditionsEditor, { props: { allOf } });
    const btn = getByTestId('rule-form-edit-condition-allof-button-0');
    expect(btn.tagName).toBe('BUTTON');
    expect(btn).toHaveTextContent('QueryParam(debug) Eq(1)');
  });

  it('clicking a condition opens ConditionForm filled with its current values', async () => {
    const { getByTestId, getByLabelText, queryByTestId } = render(RuleConditionsEditor, { props: { allOf } });

    await fireEvent.click(getByTestId('rule-form-edit-condition-allof-button-1'));

    expect(queryByTestId('rule-form-edit-condition-allof-button-1')).not.toBeInTheDocument();
    expect(getByLabelText('Source').value).toBe('Header');
    expect(getByLabelText('Key / path').value).toBe('X-Trace');
    // `Exists` has no value field.
    expect(() => getByLabelText('Expected value')).toThrow();
    // The condition that is not edited stays a button.
    expect(getByTestId('rule-form-edit-condition-allof-button-0')).toBeInTheDocument();
  });

  it('saving replaces only the edited condition and keeps the order', async () => {
    const onAllOfChange = vi.fn();
    const { getByTestId, getByLabelText, container } = render(RuleConditionsEditor, {
      props: { allOf, onAllOfChange },
    });

    await fireEvent.click(getByTestId('rule-form-edit-condition-allof-button-0'));
    await setInput(getByLabelText('Expected value'), '2');
    await fireEvent.submit(container.querySelector('form'));

    expect(onAllOfChange).toHaveBeenCalledTimes(1);
    const updated = onAllOfChange.mock.calls[0][0];
    expect(updated).toHaveLength(2);
    expect(updated[0]).toEqual({ source: { type: 'QueryParam', key: 'debug' }, operator: { type: 'Eq', value: '2' } });
    // The untouched condition stays equal to the original.
    expect(updated[1]).toEqual(allOf[1]);
  });

  it('changes the source type of an existing condition, from QueryParam to Header', async () => {
    const onAllOfChange = vi.fn();
    const { getByTestId, getByLabelText, container } = render(RuleConditionsEditor, {
      props: { allOf, onAllOfChange },
    });

    await fireEvent.click(getByTestId('rule-form-edit-condition-allof-button-0'));
    await fireEvent.change(getByLabelText('Source'), { target: { value: 'Header' } });
    await setInput(getByLabelText('Key / path'), 'X-Custom');
    await setInput(getByLabelText('Expected value'), 'yes');
    await fireEvent.submit(container.querySelector('form'));

    const updated = onAllOfChange.mock.calls[0][0];
    expect(updated[0]).toEqual({ source: { type: 'Header', key: 'X-Custom' }, operator: { type: 'Eq', value: 'yes' } });
  });

  it('cancelling the edit closes the form and changes nothing', async () => {
    const onAllOfChange = vi.fn();
    const { getByTestId, getByLabelText } = render(RuleConditionsEditor, {
      props: { allOf, onAllOfChange },
    });

    await fireEvent.click(getByTestId('rule-form-edit-condition-allof-button-0'));
    await setInput(getByLabelText('Expected value'), 'devrait-etre-ignore');
    await fireEvent.click(getByTestId('condition-form-cancel-button'));

    expect(onAllOfChange).not.toHaveBeenCalled();
    expect(getByTestId('rule-form-edit-condition-allof-button-0')).toHaveTextContent('QueryParam(debug) Eq(1)');
  });

  it('opening the add form closes an ongoing edit, so one small form is open at a time', async () => {
    const { getByTestId, getByLabelText } = render(RuleConditionsEditor, { props: { allOf } });

    await fireEvent.click(getByTestId('rule-form-edit-condition-allof-button-0'));
    await setInput(getByLabelText('Expected value'), 'en-cours-de-frappe');

    await fireEvent.click(getByTestId('rule-form-add-condition-allof-button'));
    // The edit form is gone: the condition shows its button again, with its saved label.
    expect(getByTestId('rule-form-edit-condition-allof-button-0')).toHaveTextContent('QueryParam(debug) Eq(1)');
    // The add form starts empty: the abandoned edit does not leak into it. `getByLabelText` throws on several matches,
    // so this also checks that a single form is open.
    expect(getByLabelText('Expected value').value).toBe('');
  });

  it('removing a condition before the edited one keeps the edit on the same condition', async () => {
    // The harness keeps `allOf` in `$state`, as `RuleForm.svelte` does: the array update and the shift of the edited
    // index happen in the same synchronous tick, as in production.
    const { getByTestId, getByLabelText } = render(RuleConditionsEditorHarness, {
      props: { initialAllOf: allOf },
    });

    await fireEvent.click(getByTestId('rule-form-edit-condition-allof-button-1'));
    expect(getByLabelText('Key / path').value).toBe('X-Trace');

    // The edit form stays on Header/X-Trace: it neither closes nor jumps to another condition when the indexes shift.
    await fireEvent.click(getByTestId('rule-form-remove-condition-allof-button-0'));
    expect(getByLabelText('Key / path').value).toBe('X-Trace');
  });

  it('OR conditions (`any_of`) support the same in-place edit', async () => {
    const anyOf = [{ source: { type: 'JsonPointer', key: '/a' }, operator: { type: 'Contains', value: 'x' } }];
    const onAnyOfChange = vi.fn();
    const { getByTestId, getByLabelText, container } = render(RuleConditionsEditor, {
      props: { anyOf, onAnyOfChange },
    });

    await fireEvent.click(getByTestId('rule-form-edit-condition-anyof-button-0'));
    await setInput(getByLabelText('Expected value'), 'y');
    await fireEvent.submit(container.querySelector('form'));

    expect(onAnyOfChange).toHaveBeenCalledWith([
      { source: { type: 'JsonPointer', key: '/a' }, operator: { type: 'Contains', value: 'y' } },
    ]);
  });
});
