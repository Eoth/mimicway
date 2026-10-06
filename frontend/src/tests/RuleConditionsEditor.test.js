import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RuleConditionsEditor from '../lib/components/RuleConditionsEditor.svelte';
import RuleConditionsEditorHarness from './helpers/RuleConditionsEditorHarness.svelte';

async function setInput(el, value) {
  el.value = value;
  await fireEvent.input(el);
}

describe("RuleConditionsEditor: edition en place d'une condition existante", () => {
  const allOf = [
    { source: { type: 'QueryParam', key: 'debug' }, operator: { type: 'Eq', value: '1' } },
    { source: { type: 'Header', key: 'X-Trace' }, operator: { type: 'Exists' } },
  ];

  it('affiche chaque condition comme un bouton cliquable (pas juste du texte)', () => {
    const { getByTestId } = render(RuleConditionsEditor, { props: { allOf } });
    const btn = getByTestId('rule-form-edit-condition-allof-button-0');
    expect(btn.tagName).toBe('BUTTON');
    expect(btn).toHaveTextContent('QueryParam(debug) Eq(1)');
  });

  it('cliquer sur une condition ouvre ConditionForm pre-rempli avec ses valeurs actuelles', async () => {
    const { getByTestId, getByLabelText, queryByTestId } = render(RuleConditionsEditor, { props: { allOf } });

    await fireEvent.click(getByTestId('rule-form-edit-condition-allof-button-1'));

    expect(queryByTestId('rule-form-edit-condition-allof-button-1')).not.toBeInTheDocument();
    expect(getByLabelText('Source').value).toBe('Header');
    expect(getByLabelText('Key / path').value).toBe('X-Trace');
    // Operateur "Exists" -> pas de champ valeur
    expect(() => getByLabelText('Expected value')).toThrow();
    // La condition 0, non editee, reste affichee normalement
    expect(getByTestId('rule-form-edit-condition-allof-button-0')).toBeInTheDocument();
  });

  it('enregistrer la modification remplace uniquement la condition editee, sans affecter les autres ni leur ordre', async () => {
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
    // La condition 1 (non touchee) reste strictement identique, meme reference de valeurs
    expect(updated[1]).toEqual(allOf[1]);
  });

  it("permet de changer le type de source d'une condition existante (ex. QueryParam -> Header)", async () => {
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

  it("annuler l'edition referme le formulaire sans rien modifier", async () => {
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

  it("ouvrir l'ajout referme une edition en cours (un seul mini-formulaire a la fois)", async () => {
    const { getByTestId, getByLabelText } = render(RuleConditionsEditor, { props: { allOf } });

    await fireEvent.click(getByTestId('rule-form-edit-condition-allof-button-0'));
    await setInput(getByLabelText('Expected value'), 'en-cours-de-frappe');

    await fireEvent.click(getByTestId('rule-form-add-condition-allof-button'));
    // Le formulaire d'edition de la condition 0 a disparu (remplace par celui d'ajout) :
    // la condition 0 reaffiche son bouton avec son libelle d'origine, inchange.
    expect(getByTestId('rule-form-edit-condition-allof-button-0')).toHaveTextContent('QueryParam(debug) Eq(1)');
    // Le formulaire d'ajout, lui, est vierge (pas de fuite de la saisie d'edition abandonnee)
    expect(getByLabelText('Expected value').value).toBe('');
  });

  it("supprimer une condition avant celle en cours d'edition garde l'edition alignee sur la bonne condition", async () => {
    // Utilise le harness (allOf reellement en $state, comme RuleForm.svelte) :
    // la mise a jour du tableau et l'ajustement interne de l'index edite se
    // produisent dans le meme tick synchrone, exactement comme en production.
    const { getByTestId, getByLabelText } = render(RuleConditionsEditorHarness, {
      props: { initialAllOf: allOf },
    });

    // Edite la condition d'index 1 (Header/X-Trace)
    await fireEvent.click(getByTestId('rule-form-edit-condition-allof-button-1'));
    expect(getByLabelText('Key / path').value).toBe('X-Trace');

    // Supprime la condition d'index 0 (a partir du bouton de suppression, hors edition) :
    // le formulaire d'edition doit rester sur Header/X-Trace, pas se refermer ni
    // se retrouver a editer un autre index par decalage.
    await fireEvent.click(getByTestId('rule-form-remove-condition-allof-button-0'));
    expect(getByLabelText('Key / path').value).toBe('X-Trace');
  });

  it('conditions OU (any_of) supportent la meme edition en place, independamment de ET', async () => {
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
