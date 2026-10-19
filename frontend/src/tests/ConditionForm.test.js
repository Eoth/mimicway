import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import ConditionForm from '../lib/components/ConditionForm.svelte';

async function setInput(el, value) {
  el.value = value;
  await fireEvent.input(el);
}

describe('ConditionForm: strict choice of the path param', () => {
  it('hides the "Path parameter" source when no path param is available', () => {
    const { getByLabelText } = render(ConditionForm, { props: { availablePathParams: [] } });
    const options = [...getByLabelText('Source').querySelectorAll('option')].map((o) => o.value);
    expect(options).not.toContain('PathParam');
  });

  it('shows the "Path parameter" source when path params are available', () => {
    const { getByLabelText } = render(ConditionForm, { props: { availablePathParams: ['id'] } });
    const options = [...getByLabelText('Source').querySelectorAll('option')].map((o) => o.value);
    expect(options).toContain('PathParam');
  });

  it('shows a badge with the count and names of the available path params', () => {
    const { getByText } = render(ConditionForm, { props: { availablePathParams: ['id', 'orderId', 'userId'] } });
    expect(getByText('3 path parameters available: id, orderId, userId')).toBeInTheDocument();
  });

  it('offers a closed select, not free text, for the key when PathParam is chosen', async () => {
    const { getByLabelText } = render(ConditionForm, { props: { availablePathParams: ['id', 'orderId'] } });
    const sourceSelect = getByLabelText('Source');
    await fireEvent.change(sourceSelect, { target: { value: 'PathParam' } });

    const keyField = getByLabelText('Path parameter');
    expect(keyField.tagName).toBe('SELECT');
    const options = [...keyField.querySelectorAll('option')].map((o) => o.value).filter(Boolean);
    expect(options).toEqual(['id', 'orderId']);
  });

  it('keeps the source and key of an existing PathParam condition when no path param is available', () => {
    const { getByLabelText } = render(ConditionForm, {
      props: {
        availablePathParams: [],
        condition: { source: { type: 'PathParam', key: 'legacyParam' }, operator: { type: 'Exists' } },
      },
    });
    const sourceSelect = getByLabelText('Source');
    const options = [...sourceSelect.querySelectorAll('option')].map((o) => o.value);
    expect(options).toContain('PathParam');
    const keyField = getByLabelText('Path parameter');
    const keyOptions = [...keyField.querySelectorAll('option')].map((o) => o.value).filter(Boolean);
    expect(keyOptions).toContain('legacyParam');
  });
});

describe('ConditionForm: query param autocompletion', () => {
  it('offers the suggestions in a datalist on the key input', async () => {
    const { getByLabelText, container } = render(ConditionForm, {
      props: { queryParamSuggestions: ['debug', 'trace'] },
    });
    const keyField = getByLabelText('Key / path');
    expect(keyField.getAttribute('list')).toBe('cond-query-param-suggestions');
    const datalist = container.querySelector('#cond-query-param-suggestions');
    const options = [...datalist.querySelectorAll('option')].map((o) => o.value);
    expect(options).toEqual(['debug', 'trace']);
  });

  it('still saves a query param name outside the suggestions', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container } = render(ConditionForm, {
      props: { queryParamSuggestions: ['debug'], onSave },
    });
    await setInput(getByLabelText('Key / path'), 'brand-new-param');
    await setInput(getByLabelText('Expected value'), '1');
    await fireEvent.submit(container.querySelector('form'));
    expect(onSave).toHaveBeenCalledWith({
      source: { type: 'QueryParam', key: 'brand-new-param' },
      operator: { type: 'Eq', value: '1' },
    });
  });
});

describe('ConditionForm: edit and add labels', () => {
  it('shows "OK" and the add label without a condition', () => {
    const { getByRole } = render(ConditionForm, { props: {} });
    expect(getByRole('form', { name: 'Add a condition' })).toBeInTheDocument();
    expect(getByRole('button', { name: 'OK' })).toBeInTheDocument();
  });

  it('shows "Save" and the edit label with an existing condition', () => {
    const { getByRole } = render(ConditionForm, {
      props: { condition: { source: { type: 'Header', key: 'X-Trace' }, operator: { type: 'Exists' } } },
    });
    expect(getByRole('form', { name: 'Edit the condition' })).toBeInTheDocument();
    expect(getByRole('button', { name: 'Save' })).toBeInTheDocument();
  });
});

describe('ConditionForm: visual distinction of sources', () => {
  it('uses distinct labels for PathParam and QueryParam', () => {
    const { getByLabelText } = render(ConditionForm, { props: { availablePathParams: ['id'] } });
    const optionTexts = [...getByLabelText('Source').querySelectorAll('option')].map((o) => o.textContent);
    const pathLabel = optionTexts.find((t) => t.includes('Path parameter'));
    const queryLabel = optionTexts.find((t) => t.includes('Query parameter'));
    expect(pathLabel).not.toEqual(queryLabel);
    expect(pathLabel).toContain('{param}');
    expect(queryLabel).toContain('?key=value');
  });
});
