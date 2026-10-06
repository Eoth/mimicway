import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect } from 'vitest';
import RhaiScriptEditorHarness from './helpers/RhaiScriptEditorHarness.svelte';

// Simule une frappe qui ajoute du texte a la fin (cursor place en fin de
// valeur, comme une vraie frappe clavier) : fireEvent.input ne met pas a
// jour $state en Svelte 5, on doit setter .value puis declencher
// l'evenement natif nous-memes.
async function typeAtEnd(el, text) {
  el.value = text;
  el.selectionStart = el.selectionEnd = text.length;
  await fireEvent.input(el);
}

describe('RhaiScriptEditor : autocompletion des fonctions Rhai', () => {
  it("n'affiche aucune suggestion tant que rien n'est tape", () => {
    const { queryByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    expect(queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('affiche les suggestions correspondant au prefixe tape', async () => {
    const { container, getByRole, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'seed');

    expect(getByRole('listbox')).toBeInTheDocument();
    const options = getAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual(
      expect.arrayContaining([expect.stringContaining('seeded_int'), expect.stringContaining('seeded_pick')]),
    );
  });

  it('ne montre pas de liste si le prefixe ne correspond a aucune fonction', async () => {
    const { container, queryByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'zzz');

    expect(queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('Ctrl+Espace ouvre la liste complete meme sans prefixe', async () => {
    const { container, getByRole, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');
    textarea.focus();

    await fireEvent.keyDown(textarea, { key: ' ', code: 'Space', ctrlKey: true });

    expect(getByRole('listbox')).toBeInTheDocument();
    expect(getAllByRole('option').length).toBeGreaterThan(5);
  });

  it('Echap ferme la liste de suggestions', async () => {
    const { container, queryByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'seed');
    expect(queryByRole('listbox')).toBeInTheDocument();

    await fireEvent.keyDown(textarea, { key: 'Escape' });
    expect(queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('la selection au clic insere la fonction avec parametres selectionnes', async () => {
    const { container, getAllByRole, queryByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'seeded_p');
    const option = getAllByRole('option').find((o) => o.textContent.includes('seeded_pick'));
    // mousedown (pas click) : c'est l'evenement intercepte par le
    // composant pour inserer sans perdre le focus du textarea au passage.
    await fireEvent.mouseDown(option);

    expect(textarea.value).toBe('seeded_pick(seed, ["a", "b"])');
    expect(queryByRole('listbox')).not.toBeInTheDocument();
    // Les parametres sont selectionnes pour etre remplaces par la frappe.
    expect(textarea.selectionStart).toBe('seeded_pick('.length);
    expect(textarea.selectionEnd).toBe('seeded_pick(seed, ["a", "b"]'.length);
  });

  it('la selection au clavier (fleches + Entree) insere la fonction active', async () => {
    const { container, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'seed');
    const options = getAllByRole('option');
    const seededPickIndex = options.findIndex((o) => o.textContent.includes('seeded_pick'));

    // seeded_int est actif par defaut (index 0) ; on descend jusqu'a seeded_pick.
    for (let i = 0; i < seededPickIndex; i++) {
      await fireEvent.keyDown(textarea, { key: 'ArrowDown' });
    }
    await fireEvent.keyDown(textarea, { key: 'Enter' });

    expect(textarea.value).toBe('seeded_pick(seed, ["a", "b"])');
  });

  it('insere un appel sans parametre avec le curseur juste apres', async () => {
    const { container, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'uuid');
    const option = getAllByRole('option').find((o) => o.textContent.includes('uuid()'));
    await fireEvent.mouseDown(option);

    expect(textarea.value).toBe('uuid()');
    expect(textarea.selectionStart).toBe('uuid()'.length);
    expect(textarea.selectionEnd).toBe('uuid()'.length);
  });

  it('remplace uniquement le prefixe tape, pas tout le contenu existant', async () => {
    const { container, getAllByRole } = render(RhaiScriptEditorHarness, { props: { id: 'sc' } });
    const textarea = container.querySelector('#sc');

    await typeAtEnd(textarea, 'let x = 1;\nseed');
    const option = getAllByRole('option').find((o) => o.textContent.includes('seeded_int'));
    await fireEvent.mouseDown(option);

    expect(textarea.value).toBe('let x = 1;\nseeded_int(seed, min, max)');
  });
});
