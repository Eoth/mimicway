import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import JsonResponseBuilder from '../lib/components/JsonResponseBuilder.svelte';

const nestedFields = [
  {
    key: 'unite_legale',
    fieldType: 'object',
    children: [
      { key: 'nom', fieldType: 'value', source: 'fixed', value: 'ACME', pipe: '', asNumber: false },
      {
        key: 'adresse',
        fieldType: 'object',
        children: [{ key: 'ville', fieldType: 'value', source: 'fixed', value: 'Paris', pipe: '', asNumber: false }],
      },
    ],
  },
];

describe('JsonResponseBuilder — breadcrumb navigation', () => {
  it('shows no breadcrumb at the root', () => {
    const { queryByLabelText } = render(JsonResponseBuilder, { props: { fields: nestedFields } });
    expect(queryByLabelText('Data path')).not.toBeInTheDocument();
  });

  it('renders every nesting level at the root by default', () => {
    const { getByDisplayValue } = render(JsonResponseBuilder, { props: { fields: nestedFields } });
    expect(getByDisplayValue('unite_legale')).toBeInTheDocument();
    expect(getByDisplayValue('nom')).toBeInTheDocument();
    expect(getByDisplayValue('adresse')).toBeInTheDocument();
    expect(getByDisplayValue('ville')).toBeInTheDocument();
  });

  it('the "Go into" button opens a nested object and shows the breadcrumb', async () => {
    const { getByLabelText, getByRole } = render(JsonResponseBuilder, { props: { fields: nestedFields } });

    await fireEvent.click(getByLabelText('Go into unite_legale'));

    const breadcrumb = getByRole('navigation', { name: 'Data path' });
    expect(breadcrumb).toBeInTheDocument();
    expect(breadcrumb).toHaveTextContent('root');
    expect(breadcrumb).toHaveTextContent('unite_legale');
  });

  it('clicking "root" in the breadcrumb goes back up', async () => {
    const { getByLabelText, getByText, getByDisplayValue } = render(JsonResponseBuilder, {
      props: { fields: nestedFields },
    });

    await fireEvent.click(getByLabelText('Go into unite_legale'));
    await fireEvent.click(getByLabelText('Go into adresse'));
    expect(getByDisplayValue('ville')).toBeInTheDocument();

    await fireEvent.click(getByText('root'));
    expect(getByDisplayValue('unite_legale')).toBeInTheDocument();
  });

  it('renders the last breadcrumb segment as plain text marked as the current page', async () => {
    const { getByLabelText, getByText } = render(JsonResponseBuilder, { props: { fields: nestedFields } });
    await fireEvent.click(getByLabelText('Go into unite_legale'));

    const current = getByText('unite_legale', { selector: 'span' }).closest('li');
    expect(current).toHaveAttribute('aria-current', 'page');
  });

  it('adds a new field to the focused level, not to the root', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByText } = render(JsonResponseBuilder, {
      props: { fields: nestedFields, onUpdate },
    });

    await fireEvent.click(getByLabelText('Go into unite_legale'));
    await fireEvent.click(getByText('+ Add a field'));

    const [updated] = onUpdate.mock.calls.at(-1);
    expect(updated[0].children).toHaveLength(3);
    expect(updated).toHaveLength(1);
  });
});

describe('JsonResponseBuilder — folding of nested fields', () => {
  it('unfolds everything by default', () => {
    const { getByLabelText, getByDisplayValue } = render(JsonResponseBuilder, { props: { fields: nestedFields } });
    expect(getByLabelText('Collapse unite_legale')).toHaveAttribute('aria-expanded', 'true');
    expect(getByDisplayValue('nom')).toBeVisible();
    expect(getByDisplayValue('adresse')).toBeVisible();
    expect(getByDisplayValue('ville')).toBeVisible();
  });

  it('folding a field hides its content and emits no update', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText, getByDisplayValue } = render(JsonResponseBuilder, {
      props: { fields: nestedFields, onUpdate },
    });

    const toggle = getByLabelText('Collapse unite_legale');
    await fireEvent.click(toggle);

    expect(getByLabelText('Expand unite_legale')).toHaveAttribute('aria-expanded', 'false');
    expect(getByDisplayValue('nom')).not.toBeVisible();
    expect(getByDisplayValue('adresse')).not.toBeVisible();
    expect(getByDisplayValue('ville')).not.toBeVisible();
    // Folding changes the display only, never the fields.
    expect(onUpdate).not.toHaveBeenCalled();
  });

  it('unfolding shows the hidden content again', async () => {
    const { getByLabelText, getByDisplayValue } = render(JsonResponseBuilder, { props: { fields: nestedFields } });

    await fireEvent.click(getByLabelText('Collapse unite_legale'));
    await fireEvent.click(getByLabelText('Expand unite_legale'));

    expect(getByLabelText('Collapse unite_legale')).toHaveAttribute('aria-expanded', 'true');
    expect(getByDisplayValue('nom')).toBeVisible();
    expect(getByDisplayValue('adresse')).toBeVisible();
    expect(getByDisplayValue('ville')).toBeVisible();
  });

  it('a "Value" field (without children) has no fold chevron', () => {
    const { queryByLabelText } = render(JsonResponseBuilder, { props: { fields: nestedFields } });
    expect(queryByLabelText('Collapse nom')).not.toBeInTheDocument();
    expect(queryByLabelText('Expand nom')).not.toBeInTheDocument();
  });
});

// The "Script result" source needs the value input: it names the key of the script result to read (`buildExpr` in
// tpl-utils.js writes `script.<value>`).
describe('JsonResponseBuilder — "Script result" source', () => {
  const scriptField = [{ key: 'nom', fieldType: 'value', source: 'fixed', value: '', pipe: '', asNumber: false }];

  it('shows the value input when the "script" source is chosen', async () => {
    const { getByLabelText } = render(JsonResponseBuilder, { props: { fields: scriptField } });

    await fireEvent.change(getByLabelText('Source of the value'), { target: { value: 'script' } });

    expect(getByLabelText('Value')).toBeInTheDocument();
  });

  it('passes the typed script key to onUpdate', async () => {
    const onUpdate = vi.fn();
    const { getByLabelText } = render(JsonResponseBuilder, { props: { fields: scriptField, onUpdate } });

    await fireEvent.change(getByLabelText('Source of the value'), { target: { value: 'script' } });
    await fireEvent.input(getByLabelText('Value'), { target: { value: 'total' } });

    const [updated] = onUpdate.mock.calls.at(-1);
    expect(updated[0].source).toBe('script');
    expect(updated[0].value).toBe('total');
  });
});

describe('JsonResponseBuilder: a fold follows its field', () => {
  const object = (key) => ({
    key,
    fieldType: 'object',
    children: [{ key: `${key}-child`, fieldType: 'value', source: 'fixed', value: '1', pipe: '', asNumber: false }],
  });
  const expanded = (container, testPath) =>
    container.querySelector(`[data-testid="json-builder-collapse-button-${testPath}"]`).getAttribute('aria-expanded');
  const click = (container, testId) => fireEvent.click(container.querySelector(`[data-testid="${testId}"]`));

  it('when the folded field moves down', async () => {
    const { container } = render(JsonResponseBuilder, { props: { fields: [object('first'), object('second')] } });
    await click(container, 'json-builder-collapse-button-0');
    await click(container, 'json-builder-movedown-button-0');

    expect(container.querySelector('[data-testid="json-builder-key-input-1"]').value).toBe('first');
    expect(expanded(container, '1')).toBe('false');
    expect(expanded(container, '0')).toBe('true');
  });

  it('when a field before it is deleted', async () => {
    const { container } = render(JsonResponseBuilder, { props: { fields: [object('first'), object('second')] } });
    await click(container, 'json-builder-collapse-button-1');
    await click(container, 'json-builder-delete-button-0');

    expect(container.querySelector('[data-testid="json-builder-key-input-0"]').value).toBe('second');
    expect(expanded(container, '0')).toBe('false');
  });

  it('when its parent moves, with the folds inside it', async () => {
    const parent = { key: 'parent', fieldType: 'object', children: [object('inner')] };
    const { container } = render(JsonResponseBuilder, { props: { fields: [object('first'), parent] } });
    await click(container, 'json-builder-collapse-button-1-children-0');
    await click(container, 'json-builder-moveup-button-1');

    expect(expanded(container, '0-children-0')).toBe('false');
    expect(expanded(container, '0')).toBe('true');
  });
});
