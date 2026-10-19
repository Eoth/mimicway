import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect } from 'vitest';
import RhaiScriptEditorHarness from './helpers/RhaiScriptEditorHarness.svelte';

// Types `text` with the caret at its end, like real typing: on input, the component reads both the value and the
// `selectionStart` of the textarea, so both are set before the event fires.
async function typeAtEnd(el, text) {
  el.value = text;
  el.selectionStart = el.selectionEnd = text.length;
  await fireEvent.input(el);
}

describe('RhaiScriptEditor: completion of the Rhai functions', () => {
  it('shows no suggestion before anything is typed', () => {
    const { queryByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    expect(queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('shows the suggestions that match the typed prefix', async () => {
    const { container, getByRole, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'seed');

    expect(getByRole('listbox')).toBeInTheDocument();
    const options = getAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual(
      expect.arrayContaining([expect.stringContaining('seeded_int'), expect.stringContaining('seeded_pick')]),
    );
  });

  it('shows no list when the prefix matches no function', async () => {
    const { container, queryByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'zzz');

    expect(queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('Ctrl+Space opens the list even with nothing typed', async () => {
    const { container, getByRole, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');
    textarea.focus();

    await fireEvent.keyDown(textarea, { key: ' ', code: 'Space', ctrlKey: true });

    expect(getByRole('listbox')).toBeInTheDocument();
    expect(getAllByRole('option').length).toBeGreaterThan(5);
  });

  it('Escape closes the list of suggestions', async () => {
    const { container, queryByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'seed');
    expect(queryByRole('listbox')).toBeInTheDocument();

    await fireEvent.keyDown(textarea, { key: 'Escape' });
    expect(queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('picking with the mouse inserts the function with its arguments selected', async () => {
    const { container, getAllByRole, queryByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'seeded_p');
    const option = getAllByRole('option').find((o) => o.textContent.includes('seeded_pick'));
    // mousedown, not click: the component inserts on mousedown and prevents its default, so the textarea keeps the
    // focus (a blur would close the list).
    await fireEvent.mouseDown(option);

    expect(textarea.value).toBe('seeded_pick(seed, ["a", "b"])');
    expect(queryByRole('listbox')).not.toBeInTheDocument();
    // The arguments are selected, so that typing replaces them.
    expect(textarea.selectionStart).toBe('seeded_pick('.length);
    expect(textarea.selectionEnd).toBe('seeded_pick(seed, ["a", "b"]'.length);
  });

  it('picking with the keyboard (arrows, then Enter) inserts the active function', async () => {
    const { container, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'seed');
    const options = getAllByRole('option');
    const seededPickIndex = options.findIndex((o) => o.textContent.includes('seeded_pick'));

    // The first suggestion, `seeded_int`, is active at first: move down to `seeded_pick`.
    for (let i = 0; i < seededPickIndex; i++) {
      await fireEvent.keyDown(textarea, { key: 'ArrowDown' });
    }
    await fireEvent.keyDown(textarea, { key: 'Enter' });

    expect(textarea.value).toBe('seeded_pick(seed, ["a", "b"])');
  });

  it('inserts a call without arguments with the caret right after it', async () => {
    const { container, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'uuid');
    const option = getAllByRole('option').find((o) => o.textContent.includes('uuid()'));
    await fireEvent.mouseDown(option);

    expect(textarea.value).toBe('uuid()');
    expect(textarea.selectionStart).toBe('uuid()'.length);
    expect(textarea.selectionEnd).toBe('uuid()'.length);
  });

  it('replaces only the typed prefix, not the rest of the content', async () => {
    const { container, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'let x = 1;\nseed');
    const option = getAllByRole('option').find((o) => o.textContent.includes('seeded_int'));
    await fireEvent.mouseDown(option);

    expect(textarea.value).toBe('let x = 1;\nseeded_int(seed, min, max)');
  });
});
