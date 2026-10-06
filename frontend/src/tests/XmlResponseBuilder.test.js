import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import XmlResponseBuilder from '../lib/components/XmlResponseBuilder.svelte';

const nestedFields = [
  {
    tag: 'unite_legale',
    nodeType: 'parent',
    children: [
      { tag: 'nom', nodeType: 'value', source: 'fixed', value: 'ACME' },
      {
        tag: 'adresse',
        nodeType: 'parent',
        children: [{ tag: 'ville', nodeType: 'value', source: 'fixed', value: 'Paris' }],
      },
    ],
  },
];

describe('XmlResponseBuilder — pliage/depliage des noeuds parents', () => {
  it('tout est deplie par defaut (aucune regression sur le rendu existant)', () => {
    const { getByLabelText, getByDisplayValue } = render(XmlResponseBuilder, { props: { fields: nestedFields } });
    expect(getByLabelText('Collapse unite_legale')).toHaveAttribute('aria-expanded', 'true');
    expect(getByDisplayValue('nom')).toBeVisible();
    expect(getByDisplayValue('adresse')).toBeVisible();
    expect(getByDisplayValue('ville')).toBeVisible();
  });

  it('replier un noeud parent masque ses enfants et affiche un indicateur, sans muter les donnees', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByDisplayValue } = render(XmlResponseBuilder, {
      props: { fields: nestedFields, onUpdate },
    });

    await fireEvent.click(getByLabelText('Collapse unite_legale'));

    expect(getByLabelText('Expand unite_legale')).toHaveAttribute('aria-expanded', 'false');
    expect(getByDisplayValue('nom')).not.toBeVisible();
    expect(getByDisplayValue('adresse')).not.toBeVisible();
    expect(getByDisplayValue('ville')).not.toBeVisible();
    expect(onUpdate).not.toHaveBeenCalled();
  });

  it('depliage restaure le contenu masque, aucune donnee perdue', async () => {
    const { getByLabelText, getByDisplayValue } = render(XmlResponseBuilder, { props: { fields: nestedFields } });

    await fireEvent.click(getByLabelText('Collapse unite_legale'));
    await fireEvent.click(getByLabelText('Expand unite_legale'));

    expect(getByLabelText('Collapse unite_legale')).toHaveAttribute('aria-expanded', 'true');
    expect(getByDisplayValue('nom')).toBeVisible();
    expect(getByDisplayValue('ville')).toBeVisible();
  });

  it('un noeud de type "Contenu" (sans enfants) n\'a pas de chevron de pliage', () => {
    const { queryByLabelText } = render(XmlResponseBuilder, { props: { fields: nestedFields } });
    expect(queryByLabelText('Collapse nom')).not.toBeInTheDocument();
    expect(queryByLabelText('Expand nom')).not.toBeInTheDocument();
  });
});

describe('XmlResponseBuilder — source "XPath (XML/SOAP)"', () => {
  const flatValueField = [{ tag: 'siret', nodeType: 'value', source: 'fixed', value: '' }];

  it("propose l'option XPath (XML/SOAP) dans le menu de source", () => {
    const { getByLabelText } = render(XmlResponseBuilder, { props: { fields: flatValueField } });
    const option = getByLabelText('Source').querySelector('option[value="xpath"]');
    expect(option).not.toBeNull();
    expect(option.textContent).toBe('XPath (XML/SOAP)');
  });

  it('choisir la source XPath affiche un champ Valeur avec un placeholder XPath', async () => {
    const { getByLabelText } = render(XmlResponseBuilder, { props: { fields: flatValueField } });
    await fireEvent.change(getByLabelText('Source'), { target: { value: 'xpath' } });
    expect(getByLabelText('Value')).toHaveAttribute('placeholder', 'e.g. Envelope/Body/search/Id');
  });

  it('renseigner le chemin XPath emet un onUpdate avec source xpath et la valeur saisie', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText } = render(XmlResponseBuilder, { props: { fields: flatValueField, onUpdate } });
    await fireEvent.change(getByLabelText('Source'), { target: { value: 'xpath' } });
    await fireEvent.input(getByLabelText('Value'), { target: { value: 'Envelope/Body/recherche/Siret' } });
    const lastCall = onUpdate.mock.calls.at(-1)[0];
    expect(lastCall[0]).toMatchObject({ source: 'xpath', value: 'Envelope/Body/recherche/Siret' });
  });
});

// Correctif "diagnostic reponse JSON/XML" : `needsValueInput` n'incluait pas
// 'script', contrairement au mode "par exemple" (XmlPasteBuilder.svelte) --
// choisir cette source masquait le champ qui permet de preciser QUELLE cle
// du resultat de script utiliser.
describe('XmlResponseBuilder — source "Resultat script"', () => {
  const flatValueField = [{ tag: 'nom', nodeType: 'value', source: 'fixed', value: '' }];

  it('affiche le champ de saisie de valeur quand la source "script" est choisie', async () => {
    const { getByLabelText } = render(XmlResponseBuilder, { props: { fields: flatValueField } });
    await fireEvent.change(getByLabelText('Source'), { target: { value: 'script' } });
    expect(getByLabelText('Value')).toBeInTheDocument();
  });

  it('transmet la cle du script saisie via onUpdate', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText } = render(XmlResponseBuilder, { props: { fields: flatValueField, onUpdate } });
    await fireEvent.change(getByLabelText('Source'), { target: { value: 'script' } });
    await fireEvent.input(getByLabelText('Value'), { target: { value: 'total' } });
    const lastCall = onUpdate.mock.calls.at(-1)[0];
    expect(lastCall[0]).toMatchObject({ source: 'script', value: 'total' });
  });
});

describe('XmlResponseBuilder: a fold follows its node', () => {
  const parent = (tag) => ({
    tag,
    nodeType: 'parent',
    children: [{ tag: `${tag}-child`, nodeType: 'value', source: 'fixed', value: '1' }],
  });
  const expanded = (container, testPath) =>
    container.querySelector(`[data-testid="xml-builder-collapse-button-${testPath}"]`).getAttribute('aria-expanded');

  it('when the folded node moves up', async () => {
    const { container } = render(XmlResponseBuilder, { props: { fields: [parent('first'), parent('second')] } });
    await fireEvent.click(container.querySelector('[data-testid="xml-builder-collapse-button-1"]'));
    await fireEvent.click(container.querySelector('[data-testid="xml-builder-moveup-button-1"]'));

    expect(container.querySelector('[data-testid="xml-builder-tag-input-0"]').value).toBe('second');
    expect(expanded(container, '0')).toBe('false');
    expect(expanded(container, '1')).toBe('true');
  });
});
