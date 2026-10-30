import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect } from 'vitest';
import TemplateTextInputHarness from './helpers/TemplateTextInputHarness.svelte';

// Types `text` with the caret at its end: the component reads the value and `selectionStart` on input.
async function typeAtEnd(el, text) {
  el.value = text;
  el.selectionStart = el.selectionEnd = text.length;
  await fireEvent.input(el);
}

function setup(props = {}) {
  const view = render(TemplateTextInputHarness, { props });
  const input = view.getByTestId('tti');
  return { ...view, input };
}

describe('TemplateTextInput: a free text with values of the request', () => {
  it('suggests the values after {{ and inserts the picked one, closed', async () => {
    const { input, getByRole, getByTestId, queryByRole } = setup();
    await fireEvent.focus(input);
    await typeAtEnd(input, 'Id {{uu');
    expect(getByRole('listbox')).toBeInTheDocument();
    expect(input.getAttribute('aria-activedescendant')).toBeTruthy();

    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(getByTestId('tti-value').textContent).toBe('Id {{uuid}}');
    expect(queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('suggests the pipes after a bar, picked with the mouse', async () => {
    const { input, getByTestId } = setup();
    await fireEvent.focus(input);
    await typeAtEnd(input, '{{query.name | up');
    await fireEvent.mouseDown(getByTestId('tti-suggestion-upper'));
    expect(getByTestId('tti-value').textContent).toBe('{{query.name | upper}}');
  });

  it('names an expression the engine would render empty, tied to the input', async () => {
    const { input, getByTestId } = setup({ initial: 'Hi {{qery.name}}' });
    const notes = getByTestId('tti-notes');
    expect(notes.textContent).toContain('{{qery.name}}');
    expect(input.getAttribute('aria-describedby')).toBe(notes.id);
  });

  it('shows how to start while focused, and nothing for a plain text otherwise', async () => {
    const { input, getByTestId, queryByTestId } = setup({ initial: 'active' });
    expect(queryByTestId('tti-tip')).not.toBeInTheDocument();
    await fireEvent.focus(input);
    expect(getByTestId('tti-tip').textContent).toContain('{{');
    expect(input.getAttribute('aria-describedby')).toBe(getByTestId('tti-tip').id);
  });
});

describe('TemplateTextInput: a pipe chain', () => {
  it('lists every pipe on focus, and Escape closes the list', async () => {
    const { input, getAllByRole, queryByRole } = setup({ kind: 'pipe' });
    await fireEvent.focus(input);
    expect(getAllByRole('option').length).toBeGreaterThan(10);
    await fireEvent.keyDown(input, { key: 'Escape' });
    expect(queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('shows what a picked pipe does, and warns about an unknown one', async () => {
    const { input, getByTestId } = setup({ kind: 'pipe' });
    await fireEvent.focus(input);
    await typeAtEnd(input, 'fir');
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(getByTestId('tti-value').textContent).toBe('first(3)');
    expect(getByTestId('tti-tip').textContent).toContain('“abcdef” → “abc”');

    await typeAtEnd(input, 'uper');
    expect(getByTestId('tti-notes').textContent).toContain('uper');
  });

  it('says why a fixed value that quotes cannot hold is sent unchanged', () => {
    const { getByTestId } = setup({ kind: 'pipe', initial: 'upper', fixedValue: 'a"b' });
    expect(getByTestId('tti-notes').textContent).toContain('plain text');
  });
});
