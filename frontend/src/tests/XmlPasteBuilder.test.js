import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import XmlPasteBuilder from '../lib/components/XmlPasteBuilder.svelte';

async function pasteAndParse(getByLabelText, getByText, xml) {
  const textarea = getByLabelText('Paste an example of the XML response');
  await fireEvent.input(textarea, { target: { value: xml } });
  await fireEvent.click(getByText('Analyze and make it variable'));
}

describe('XmlPasteBuilder (exampleXmlToFields via tpl-utils.js)', () => {
  it("detecte les noeuds d'un XML colle et les affiche", async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(XmlPasteBuilder, { props: { onUpdate } });

    await pasteAndParse(getByLabelText, getByText, '<response><siret>44306184100047</siret><nom>ACME</nom></response>');

    await waitFor(() => expect(getByText('siret')).toBeInTheDocument());
    expect(getByText('nom')).toBeInTheDocument();
    expect(onUpdate).toHaveBeenCalled();
  });

  it('affiche le tag racine detecte', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<devis><id>1</id></devis>');
    await waitFor(() => expect(getByText('devis', { exact: false })).toBeInTheDocument());
  });

  it('affiche une erreur sur un XML invalide', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<a><b></a>');
    await waitFor(() =>
      expect(getByText('Invalid XML: check the tags (empty names, wrong nesting).')).toBeInTheDocument(),
    );
  });

  it('rejette une racine sans element imbrique avec un message explicite', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<response>just text</response>');
    await waitFor(() =>
      expect(
        getByText('The XML root holds no nested element. Paste XML with at least one child element.'),
      ).toBeInTheDocument(),
    );
  });

  it('le bouton "Recoller un XML" revient a la zone de collage', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<r><a>1</a></r>');
    await waitFor(() => expect(getByText('a')).toBeInTheDocument());

    await fireEvent.click(getByText('Paste another XML'));
    expect(getByLabelText('Paste an example of the XML response')).toBeInTheDocument();
  });

  it('assigner une source transforme la previsualisation en variable', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(XmlPasteBuilder, { props: { onUpdate } });
    await pasteAndParse(getByLabelText, getByText, '<r><siret>123</siret></r>');
    await waitFor(() => expect(getByText('siret')).toBeInTheDocument());

    const select = getByLabelText('Source for siret');
    await fireEvent.change(select, { target: { value: 'path' } });

    const [fields] = onUpdate.mock.calls.at(-1);
    expect(fields[0].source).toBe('path');
  });
});

describe("XmlPasteBuilder — navigation par fil d'Ariane et pliage (divergence assumee vs JsonPasteBuilder)", () => {
  const nestedXml = '<r><unite_legale><nom>ACME</nom><adresse><ville>Paris</ville></adresse></unite_legale></r>';

  it("n'affiche pas de breadcrumb tant qu'on est a la racine", async () => {
    const { getByLabelText, getByText, queryByLabelText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedXml);
    await waitFor(() => expect(getByText('unite_legale')).toBeInTheDocument());
    expect(queryByLabelText('Data path')).not.toBeInTheDocument();
  });

  it('navigue dans un noeud parent via le bouton "Naviguer"', async () => {
    const { getByLabelText, getByText, getByRole } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedXml);
    await waitFor(() => expect(getByLabelText('Go into unite_legale')).toBeInTheDocument());

    await fireEvent.click(getByLabelText('Go into unite_legale'));

    const breadcrumb = getByRole('navigation', { name: 'Data path' });
    expect(breadcrumb).toHaveTextContent('root');
    expect(breadcrumb).toHaveTextContent('unite_legale');
    expect(getByText('nom')).toBeInTheDocument();
  });

  it('permet de remonter en cliquant sur "racine" dans le breadcrumb', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedXml);
    await waitFor(() => expect(getByLabelText('Go into unite_legale')).toBeInTheDocument());

    await fireEvent.click(getByLabelText('Go into unite_legale'));
    expect(getByText('nom')).toBeInTheDocument();

    await fireEvent.click(getByText('root'));
    expect(getByText('unite_legale')).toBeInTheDocument();
  });

  it('replier un noeud masque ses enfants et affiche un indicateur, sans muter les donnees', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText, queryByText } = render(XmlPasteBuilder, { props: { onUpdate } });
    await pasteAndParse(getByLabelText, getByText, nestedXml);
    await waitFor(() => expect(getByLabelText('Collapse unite_legale')).toBeInTheDocument());
    onUpdate.mockClear();

    await fireEvent.click(getByLabelText('Collapse unite_legale'));

    expect(getByLabelText('Expand unite_legale')).toHaveAttribute('aria-expanded', 'false');
    expect(queryByText('nom')).not.toBeVisible();
    expect(onUpdate).not.toHaveBeenCalled();
  });

  it('un noeud "value" (sans enfants) n\'a pas de chevron de pliage', async () => {
    const { getByLabelText, getByText, queryByLabelText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedXml);
    await waitFor(() => expect(getByLabelText('Go into unite_legale')).toBeInTheDocument());
    await fireEvent.click(getByLabelText('Go into unite_legale'));
    expect(queryByLabelText('Collapse nom')).not.toBeInTheDocument();
  });
});

describe('XmlPasteBuilder — attributs XML (specificite absente du modele JSON)', () => {
  it("detecte et affiche les attributs de la racine et d'un noeud enfant", async () => {
    const { getByLabelText, getByText, getAllByText } = render(XmlPasteBuilder);
    await pasteAndParse(
      getByLabelText,
      getByText,
      '<response xmlns:soap="http://x"><id type="uuid">42</id></response>',
    );

    await waitFor(() => expect(getByText('@xmlns:soap')).toBeInTheDocument());
    expect(getByText('@type')).toBeInTheDocument();
    expect(getAllByText('Attributes:')).toHaveLength(2);
  });

  it('assigner une source a un attribut le transforme en variable, sans toucher au contenu du noeud', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(XmlPasteBuilder, { props: { onUpdate } });
    await pasteAndParse(getByLabelText, getByText, '<r><id type="fixe">42</id></r>');
    await waitFor(() => expect(getByText('@type')).toBeInTheDocument());

    const attrSelect = getByLabelText('Source for the attribute type (0)');
    await fireEvent.change(attrSelect, { target: { value: 'header' } });

    const [fields] = onUpdate.mock.calls.at(-1);
    expect(fields[0].attributes[0].source).toBe('header');
    expect(fields[0].value).toBe('42');
  });

  it('ne plante pas sur un XML sans attributs (liste vide, pas de section Attributs affichee)', async () => {
    const { getByLabelText, getByText, queryByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<r><a>1</a></r>');
    await waitFor(() => expect(getByText('a')).toBeInTheDocument());
    expect(queryByText('Attributes:')).not.toBeInTheDocument();
  });
});

// startParsed/rootTag/rootAttributes : seedent l'etat initial pour la
// restauration d'une regle existante, miroir de JsonPasteBuilder.test.js.
describe("XmlPasteBuilder — startParsed/rootTag (restauration a l'edition, retour 1)", () => {
  it('affiche directement la liste de noeuds quand startParsed=true, sans repasser par la zone de collage', () => {
    const fields = [{ tag: 'siret', nodeType: 'value', source: 'path', value: 'siret', pipe: '', attributes: [] }];
    const { getByText, queryByLabelText } = render(XmlPasteBuilder, {
      props: { fields, startParsed: true, rootTag: 'devisResponse', rootAttributes: [] },
    });

    expect(getByText('siret')).toBeInTheDocument();
    expect(queryByLabelText('Paste an example of the XML response')).not.toBeInTheDocument();
  });

  it('affiche la zone de collage quand startParsed=false (defaut), meme avec des fields fournis', () => {
    const fields = [{ tag: 'siret', nodeType: 'value', source: 'fixed', value: '123', pipe: '', attributes: [] }];
    const { getByLabelText, queryByText } = render(XmlPasteBuilder, { props: { fields } });

    expect(getByLabelText('Paste an example of the XML response')).toBeInTheDocument();
    expect(queryByText('siret')).not.toBeInTheDocument();
  });
});

// Pipes (retour 2) : sur le contenu d'un noeud valeur uniquement (pas les
// attributs, cf commentaire du composant).
describe('XmlPasteBuilder — pipes (retour 2)', () => {
  it('n\'affiche pas de champ pipe pour une source "fixed"', async () => {
    const { getByLabelText, getByText, queryByLabelText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<r><nom>ACME</nom></r>');
    await waitFor(() => expect(getByText('nom')).toBeInTheDocument());
    expect(queryByLabelText('Transformation pipe for nom')).not.toBeInTheDocument();
  });

  it("affiche un champ pipe des qu'une source non-fixe est choisie, et le transmet via onUpdate", async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(XmlPasteBuilder, { props: { onUpdate } });
    await pasteAndParse(getByLabelText, getByText, '<r><siret>123</siret></r>');
    await waitFor(() => expect(getByText('siret')).toBeInTheDocument());

    await fireEvent.change(getByLabelText('Source for siret'), { target: { value: 'path' } });
    const pipeInput = getByLabelText('Transformation pipe for siret');
    await fireEvent.input(pipeInput, { target: { value: 'upper' } });

    const [fields] = onUpdate.mock.calls.at(-1);
    expect(fields[0].pipe).toBe('upper');
  });
});
