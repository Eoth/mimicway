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

describe('XmlResponseBuilder — folding of parent nodes', () => {
  it('unfolds everything by default', () => {
    const { getByLabelText, getByDisplayValue } = render(XmlResponseBuilder, { props: { fields: nestedFields } });
    expect(getByLabelText('Collapse unite_legale')).toHaveAttribute('aria-expanded', 'true');
    expect(getByDisplayValue('nom')).toBeVisible();
    expect(getByDisplayValue('adresse')).toBeVisible();
    expect(getByDisplayValue('ville')).toBeVisible();
  });

  it('folding a parent node hides its children and emits no update', async () => {
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

  it('unfolding shows the hidden content again', async () => {
    const { getByLabelText, getByDisplayValue } = render(XmlResponseBuilder, { props: { fields: nestedFields } });

    await fireEvent.click(getByLabelText('Collapse unite_legale'));
    await fireEvent.click(getByLabelText('Expand unite_legale'));

    expect(getByLabelText('Collapse unite_legale')).toHaveAttribute('aria-expanded', 'true');
    expect(getByDisplayValue('nom')).toBeVisible();
    expect(getByDisplayValue('ville')).toBeVisible();
  });

  it('a "Content" node (without children) has no fold chevron', () => {
    const { queryByLabelText } = render(XmlResponseBuilder, { props: { fields: nestedFields } });
    expect(queryByLabelText('Collapse nom')).not.toBeInTheDocument();
    expect(queryByLabelText('Expand nom')).not.toBeInTheDocument();
  });
});

describe('XmlResponseBuilder — source "XPath (XML/SOAP)"', () => {
  const flatValueField = [{ tag: 'siret', nodeType: 'value', source: 'fixed', value: '' }];

  it('offers the XPath (XML/SOAP) option in the source menu', () => {
    const { getByLabelText } = render(XmlResponseBuilder, { props: { fields: flatValueField } });
    const option = getByLabelText('Source').querySelector('option[value="xpath"]');
    expect(option).not.toBeNull();
    expect(option.textContent).toBe('XPath (XML/SOAP)');
  });

  it('choosing the XPath source shows a Value input with an XPath placeholder', async () => {
    const { getByLabelText } = render(XmlResponseBuilder, { props: { fields: flatValueField } });
    await fireEvent.change(getByLabelText('Source'), { target: { value: 'xpath' } });
    expect(getByLabelText('Value')).toHaveAttribute('placeholder', 'e.g. Envelope/Body/search/Id');
  });

  it('typing the XPath path sends onUpdate the xpath source and the typed value', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText } = render(XmlResponseBuilder, { props: { fields: flatValueField, onUpdate } });
    await fireEvent.change(getByLabelText('Source'), { target: { value: 'xpath' } });
    await fireEvent.input(getByLabelText('Value'), { target: { value: 'Envelope/Body/recherche/Siret' } });
    const lastCall = onUpdate.mock.calls.at(-1)[0];
    expect(lastCall[0]).toMatchObject({ source: 'xpath', value: 'Envelope/Body/recherche/Siret' });
  });
});

// The "Script result" source needs the value input: it names the key of the script result to read.
describe('XmlResponseBuilder — "Script result" source', () => {
  const flatValueField = [{ tag: 'nom', nodeType: 'value', source: 'fixed', value: '' }];

  it('shows the value input when the "script" source is chosen', async () => {
    const { getByLabelText } = render(XmlResponseBuilder, { props: { fields: flatValueField } });
    await fireEvent.change(getByLabelText('Source'), { target: { value: 'script' } });
    expect(getByLabelText('Value')).toBeInTheDocument();
  });

  it('passes the typed script key to onUpdate', async () => {
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
