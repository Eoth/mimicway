import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import JsonPasteBuilder from '../lib/components/JsonPasteBuilder.svelte';

async function pasteAndParse(getByLabelText, getByText, json) {
  const textarea = getByLabelText('Paste an example of the JSON response');
  await fireEvent.input(textarea, { target: { value: json } });
  await fireEvent.click(getByText('Analyze and make it variable'));
}

describe('JsonPasteBuilder (exampleJsonToFields via tpl-utils.js)', () => {
  it("detecte les champs d'un objet colle et les affiche", async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(JsonPasteBuilder, { props: { onUpdate } });

    await pasteAndParse(getByLabelText, getByText, '{"siret":"44306184100047","nom":"ACME"}');

    await waitFor(() => expect(getByText('siret')).toBeInTheDocument());
    expect(getByText('nom')).toBeInTheDocument();
    expect(onUpdate).toHaveBeenCalled();
  });

  it('supporte un tableau racine', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(JsonPasteBuilder, { props: { onUpdate } });

    await pasteAndParse(getByLabelText, getByText, '[{"id":1},{"id":2}]');

    await waitFor(() => expect(getByText('id')).toBeInTheDocument());
    const [fields] = onUpdate.mock.calls.at(-1);
    expect(fields[0].value).toBe('1');
  });

  it('affiche une erreur sur un JSON invalide', async () => {
    const { getByLabelText, getByText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '{invalid');
    await waitFor(() => expect(getByText(/^Invalid JSON: ./)).toBeInTheDocument());
  });

  it('rejette un tableau vide avec un message explicite', async () => {
    const { getByLabelText, getByText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '[]');
    await waitFor(() =>
      expect(getByText('The array is empty. Paste an array with at least one element.')).toBeInTheDocument(),
    );
  });
});

// startParsed : seede l'etat initial pour la restauration d'une regle
// existante. Sans cette prop (comportement par defaut, teste ci-dessus), le
// composant affiche toujours la zone de collage en premier, meme avec des
// `fields` deja peuples -- exactement le comportement a eviter a la
// restauration.
describe("JsonPasteBuilder — startParsed (restauration a l'edition, retour 1)", () => {
  it('affiche directement la liste de champs quand startParsed=true et fields deja peuple', () => {
    const fields = [{ key: 'siret', fieldType: 'value', source: 'path', value: 'siret', pipe: '', asNumber: false }];
    const { getByText, queryByLabelText } = render(JsonPasteBuilder, { props: { fields, startParsed: true } });

    expect(getByText('siret')).toBeInTheDocument();
    expect(queryByLabelText('Paste an example of the JSON response')).not.toBeInTheDocument();
  });

  it('affiche la zone de collage quand startParsed=false (defaut), meme avec des fields fournis', () => {
    const fields = [{ key: 'siret', fieldType: 'value', source: 'fixed', value: '123', pipe: '', asNumber: false }];
    const { getByLabelText, queryByText } = render(JsonPasteBuilder, { props: { fields } });

    expect(getByLabelText('Paste an example of the JSON response')).toBeInTheDocument();
    expect(queryByText('siret')).not.toBeInTheDocument();
  });
});

// Pliage des champs 'object' (correctif "diagnostic reponse JSON/XML") : ce
// mode "par exemple" en etait prive par oubli, alors que son equivalent XML
// (XmlPasteBuilder) l'a des l'origine. Contrairement a XmlPasteBuilder, ce
// composant n'a pas de fil d'Ariane (rendu recursif a plat, divergence
// assumee) -- seul le pliage est ajoute ici.
describe('JsonPasteBuilder — pliage des champs objet (correctif diagnostic)', () => {
  const nestedJson = '{"client":{"nom":"ACME","siret":"123"}}';

  it('tout est deplie par defaut', async () => {
    const { getByLabelText, getByText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedJson);
    await waitFor(() => expect(getByText('client')).toBeInTheDocument());
    expect(getByText('nom')).toBeInTheDocument();
    expect(getByText('siret')).toBeInTheDocument();
  });

  it('replier un champ objet masque ses enfants et affiche un indicateur, sans muter les donnees', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText, queryByText } = render(JsonPasteBuilder, { props: { onUpdate } });
    await pasteAndParse(getByLabelText, getByText, nestedJson);
    await waitFor(() => expect(getByLabelText('Collapse client')).toBeInTheDocument());
    onUpdate.mockClear();

    await fireEvent.click(getByLabelText('Collapse client'));

    expect(getByLabelText('Expand client')).toHaveAttribute('aria-expanded', 'false');
    expect(queryByText('nom')).not.toBeVisible();
    expect(getByText('(2 hidden items)')).toBeInTheDocument();
    expect(onUpdate).not.toHaveBeenCalled();
  });

  it("deplier restaure l'affichage des enfants", async () => {
    const { getByLabelText, getByText, queryByText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedJson);
    await waitFor(() => expect(getByLabelText('Collapse client')).toBeInTheDocument());

    await fireEvent.click(getByLabelText('Collapse client'));
    expect(queryByText('nom')).not.toBeVisible();

    await fireEvent.click(getByLabelText('Expand client'));
    expect(getByText('nom')).toBeVisible();
  });

  it('un champ "valeur" simple n\'a pas de chevron de pliage', async () => {
    const { getByLabelText, getByText, queryByLabelText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '{"nom":"ACME"}');
    await waitFor(() => expect(getByText('nom')).toBeInTheDocument());
    expect(queryByLabelText('Collapse nom')).not.toBeInTheDocument();
  });
});

// Pipes : absents avant cette passe, ajoutes en coherence avec le mode
// guide (JsonResponseBuilder.svelte). Visible uniquement pour une source
// non-fixe (meme regle que le mode guide).
describe('JsonPasteBuilder — pipes (retour 2)', () => {
  it('n\'affiche pas de champ pipe pour une source "fixed"', async () => {
    const { getByLabelText, getByText, queryByLabelText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '{"nom":"ACME"}');
    await waitFor(() => expect(getByText('nom')).toBeInTheDocument());
    expect(queryByLabelText('Transformation pipe for nom')).not.toBeInTheDocument();
  });

  it("affiche un champ pipe des qu'une source non-fixe est choisie, et le transmet via onUpdate", async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(JsonPasteBuilder, { props: { onUpdate } });
    await pasteAndParse(getByLabelText, getByText, '{"siret":"123"}');
    await waitFor(() => expect(getByText('siret')).toBeInTheDocument());

    await fireEvent.change(getByLabelText('Source for siret'), { target: { value: 'path' } });
    const pipeInput = getByLabelText('Transformation pipe for siret');
    await fireEvent.input(pipeInput, { target: { value: 'upper' } });

    const [fields] = onUpdate.mock.calls.at(-1);
    expect(fields[0].pipe).toBe('upper');
  });
});
