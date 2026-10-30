import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import XmlPasteBuilder from '../lib/components/XmlPasteBuilder.svelte';

async function pasteAndParse(getByLabelText, getByText, xml) {
  const textarea = getByLabelText('Paste an example of the XML response');
  await fireEvent.input(textarea, { target: { value: xml } });
  await fireEvent.click(getByText('Analyze and make it variable'));
}

describe('XmlPasteBuilder (exampleXmlToFields via tpl-utils.js)', () => {
  it('detects and shows the nodes of pasted XML', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(XmlPasteBuilder, { props: { onUpdate } });

    await pasteAndParse(getByLabelText, getByText, '<response><siret>44306184100047</siret><nom>ACME</nom></response>');

    await waitFor(() => expect(getByText('siret')).toBeInTheDocument());
    expect(getByText('nom')).toBeInTheDocument();
    expect(onUpdate).toHaveBeenCalled();
  });

  it('shows the detected root tag', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<devis><id>1</id></devis>');
    await waitFor(() => expect(getByText('devis', { exact: false })).toBeInTheDocument());
  });

  it('shows an error on invalid XML', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<a><b></a>');
    await waitFor(() =>
      expect(getByText('Invalid XML: check the tags (empty names, wrong nesting).')).toBeInTheDocument(),
    );
  });

  it('rejects a root without a child element with a message that says so', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<response>just text</response>');
    await waitFor(() =>
      expect(
        getByText('The XML root holds no nested element. Paste XML with at least one child element.'),
      ).toBeInTheDocument(),
    );
  });

  it('the "Paste another XML" button goes back to the paste area', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<r><a>1</a></r>');
    await waitFor(() => expect(getByText('a')).toBeInTheDocument());

    await fireEvent.click(getByText('Paste another XML'));
    expect(getByLabelText('Paste an example of the XML response')).toBeInTheDocument();
  });

  it('choosing a source for a node passes it to onUpdate', async () => {
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

// Of the two paste views, only this one has a breadcrumb: pasted XML, often a SOAP envelope, nests much deeper than
// JSON.
describe('XmlPasteBuilder — breadcrumb navigation and folding', () => {
  const nestedXml = '<r><unite_legale><nom>ACME</nom><adresse><ville>Paris</ville></adresse></unite_legale></r>';

  it('shows no breadcrumb at the root', async () => {
    const { getByLabelText, getByText, queryByLabelText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedXml);
    await waitFor(() => expect(getByText('unite_legale')).toBeInTheDocument());
    expect(queryByLabelText('Data path')).not.toBeInTheDocument();
  });

  it('the "Go into" button opens a parent node and shows the breadcrumb', async () => {
    const { getByLabelText, getByText, getByRole } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedXml);
    await waitFor(() => expect(getByLabelText('Go into unite_legale')).toBeInTheDocument());

    await fireEvent.click(getByLabelText('Go into unite_legale'));

    const breadcrumb = getByRole('navigation', { name: 'Data path' });
    expect(breadcrumb).toHaveTextContent('root');
    expect(breadcrumb).toHaveTextContent('unite_legale');
    expect(getByText('nom')).toBeInTheDocument();
  });

  it('clicking "root" in the breadcrumb goes back up', async () => {
    const { getByLabelText, getByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedXml);
    await waitFor(() => expect(getByLabelText('Go into unite_legale')).toBeInTheDocument());

    await fireEvent.click(getByLabelText('Go into unite_legale'));
    expect(getByText('nom')).toBeInTheDocument();

    await fireEvent.click(getByText('root'));
    expect(getByText('unite_legale')).toBeInTheDocument();
  });

  it('folding a node hides its children and emits no update', async () => {
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

  it('a "value" node (without children) has no fold chevron', async () => {
    const { getByLabelText, getByText, queryByLabelText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedXml);
    await waitFor(() => expect(getByLabelText('Go into unite_legale')).toBeInTheDocument());
    await fireEvent.click(getByLabelText('Go into unite_legale'));
    expect(queryByLabelText('Collapse nom')).not.toBeInTheDocument();
  });
});

describe('XmlPasteBuilder — XML attributes', () => {
  it('detects and shows the attributes of the root and of a child node', async () => {
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

  it('choosing a source for an attribute leaves the content of its node unchanged', async () => {
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

  it('shows no Attributes section for XML without attributes', async () => {
    const { getByLabelText, getByText, queryByText } = render(XmlPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '<r><a>1</a></r>');
    await waitFor(() => expect(getByText('a')).toBeInTheDocument());
    expect(queryByText('Attributes:')).not.toBeInTheDocument();
  });
});

// `startParsed`, `rootTag` and `rootAttributes` restore an existing rule, as in JsonPasteBuilder.test.js.
describe('XmlPasteBuilder — startParsed/rootTag (restoring a saved rule)', () => {
  it('shows the list of nodes and the root tag at once when startParsed=true, without the paste area', () => {
    const fields = [{ tag: 'siret', nodeType: 'value', source: 'path', value: 'siret', pipe: '', attributes: [] }];
    const { getByText, queryByLabelText } = render(XmlPasteBuilder, {
      props: { fields, startParsed: true, rootTag: 'devisResponse', rootAttributes: [] },
    });

    expect(getByText('siret')).toBeInTheDocument();
    expect(getByText('<devisResponse>')).toBeInTheDocument();
    expect(queryByLabelText('Paste an example of the XML response')).not.toBeInTheDocument();
  });

  it('shows the paste area when startParsed=false (the default), even with fields given', () => {
    const fields = [{ tag: 'siret', nodeType: 'value', source: 'fixed', value: '123', pipe: '', attributes: [] }];
    const { getByLabelText, queryByText } = render(XmlPasteBuilder, { props: { fields } });

    expect(getByLabelText('Paste an example of the XML response')).toBeInTheDocument();
    expect(queryByText('siret')).not.toBeInTheDocument();
  });
});

// A pipe applies to the content of a value node only: attributes take a source but no pipe.
describe('XmlPasteBuilder — pipes', () => {
  it('offers a pipe for a kept value too, which then reads as quoted text in the template', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(XmlPasteBuilder, { props: { onUpdate } });
    await pasteAndParse(getByLabelText, getByText, '<r><nom>ACME</nom></r>');
    await waitFor(() => expect(getByText('nom')).toBeInTheDocument());

    await fireEvent.input(getByLabelText('Transformation pipe for nom'), { target: { value: 'lower' } });

    const [fields] = onUpdate.mock.calls.at(-1);
    expect(fields[0]).toMatchObject({ source: 'fixed', value: 'ACME', pipe: 'lower' });
    expect(getByText('{{"ACME" | lower}}')).toBeInTheDocument();
  });

  it('shows a pipe input once another source is chosen and passes the pipe to onUpdate', async () => {
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
