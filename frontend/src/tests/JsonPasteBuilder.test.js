import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import JsonPasteBuilder from '../lib/components/JsonPasteBuilder.svelte';

async function pasteAndParse(getByLabelText, getByText, json) {
  const textarea = getByLabelText('Paste an example of the JSON response');
  await fireEvent.input(textarea, { target: { value: json } });
  await fireEvent.click(getByText('Analyze and make it variable'));
}

describe('JsonPasteBuilder (exampleJsonToFields via tpl-utils.js)', () => {
  it('detects and shows the fields of a pasted object', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(JsonPasteBuilder, { props: { onUpdate } });

    await pasteAndParse(getByLabelText, getByText, '{"siret":"44306184100047","nom":"ACME"}');

    await waitFor(() => expect(getByText('siret')).toBeInTheDocument());
    expect(getByText('nom')).toBeInTheDocument();
    expect(onUpdate).toHaveBeenCalled();
  });

  it('takes the fields of the first element of a root array', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(JsonPasteBuilder, { props: { onUpdate } });

    await pasteAndParse(getByLabelText, getByText, '[{"id":1},{"id":2}]');

    await waitFor(() => expect(getByText('id')).toBeInTheDocument());
    const [fields] = onUpdate.mock.calls.at(-1);
    expect(fields[0].value).toBe('1');
  });

  it('shows an error on invalid JSON', async () => {
    const { getByLabelText, getByText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '{invalid');
    await waitFor(() => expect(getByText(/^Invalid JSON: ./)).toBeInTheDocument());
  });

  it('rejects an empty array with a message that says so', async () => {
    const { getByLabelText, getByText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '[]');
    await waitFor(() =>
      expect(getByText('The array is empty. Paste an array with at least one element.')).toBeInTheDocument(),
    );
  });
});

// `startParsed` restores an existing rule: without it, the component always opens on the paste area, even when
// `fields` is already filled.
describe('JsonPasteBuilder — startParsed (restoring a saved rule)', () => {
  it('shows the list of fields at once when startParsed=true and fields is filled', () => {
    const fields = [{ key: 'siret', fieldType: 'value', source: 'path', value: 'siret', pipe: '', asNumber: false }];
    const { getByText, queryByLabelText } = render(JsonPasteBuilder, { props: { fields, startParsed: true } });

    expect(getByText('siret')).toBeInTheDocument();
    expect(queryByLabelText('Paste an example of the JSON response')).not.toBeInTheDocument();
  });

  it('shows the paste area when startParsed=false (the default), even with fields given', () => {
    const fields = [{ key: 'siret', fieldType: 'value', source: 'fixed', value: '123', pipe: '', asNumber: false }];
    const { getByLabelText, queryByText } = render(JsonPasteBuilder, { props: { fields } });

    expect(getByLabelText('Paste an example of the JSON response')).toBeInTheDocument();
    expect(queryByText('siret')).not.toBeInTheDocument();
  });
});

// Unlike XmlPasteBuilder, this view has no breadcrumb: it renders the whole tree, so folding is its only way to
// shorten it.
describe('JsonPasteBuilder — folding of object fields', () => {
  const nestedJson = '{"client":{"nom":"ACME","siret":"123"}}';

  it('unfolds everything by default', async () => {
    const { getByLabelText, getByText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedJson);
    await waitFor(() => expect(getByText('client')).toBeInTheDocument());
    expect(getByText('nom')).toBeInTheDocument();
    expect(getByText('siret')).toBeInTheDocument();
  });

  it('folding an object field hides its children, shows a count and emits no update', async () => {
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

  it('unfolding shows the children again', async () => {
    const { getByLabelText, getByText, queryByText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, nestedJson);
    await waitFor(() => expect(getByLabelText('Collapse client')).toBeInTheDocument());

    await fireEvent.click(getByLabelText('Collapse client'));
    expect(queryByText('nom')).not.toBeVisible();

    await fireEvent.click(getByLabelText('Expand client'));
    expect(getByText('nom')).toBeVisible();
  });

  it('a plain value field has no fold chevron', async () => {
    const { getByLabelText, getByText, queryByLabelText } = render(JsonPasteBuilder);
    await pasteAndParse(getByLabelText, getByText, '{"nom":"ACME"}');
    await waitFor(() => expect(getByText('nom')).toBeInTheDocument());
    expect(queryByLabelText('Collapse nom')).not.toBeInTheDocument();
  });
});

// As in JsonResponseBuilder, the pipe input shows only for a source other than 'fixed'.
describe('JsonPasteBuilder — pipes', () => {
  it('offers a pipe for a kept value too, which then reads as quoted text in the template', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(JsonPasteBuilder, { props: { onUpdate } });
    await pasteAndParse(getByLabelText, getByText, '{"nom":"ACME"}');
    await waitFor(() => expect(getByText('nom')).toBeInTheDocument());

    await fireEvent.input(getByLabelText('Transformation pipe for nom'), { target: { value: 'lower' } });

    const [fields] = onUpdate.mock.calls.at(-1);
    expect(fields[0]).toMatchObject({ source: 'fixed', value: 'ACME', pipe: 'lower' });
    expect(getByText('{{"ACME" | lower}}')).toBeInTheDocument();
  });

  it('shows a pipe input once another source is chosen and passes the pipe to onUpdate', async () => {
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
