import { render } from '@testing-library/svelte';
import { describe, it, expect } from 'vitest';
import FormFieldHarness from './helpers/FormFieldHarness.svelte';

describe('FormField', () => {
  it('ties the label to the field through `for` and `id`', () => {
    const { getByLabelText } = render(FormFieldHarness, {
      props: { id: 'svc-name', label: 'Service name' },
    });
    expect(getByLabelText('Service name')).toBeInTheDocument();
  });

  it('links the hint through `aria-describedby`', () => {
    const { getByLabelText, getByText } = render(FormFieldHarness, {
      props: { id: 'svc-name', label: 'Service name', hint: 'Unique identifier' },
    });
    const input = getByLabelText('Service name');
    const hint = getByText('Unique identifier');
    expect(input.getAttribute('aria-describedby')).toContain(hint.id);
  });

  it('shows the error as an alert and sets `aria-invalid` on the field', () => {
    const { getByLabelText, getByRole } = render(FormFieldHarness, {
      props: { id: 'svc-name', label: 'Service name', error: 'The name is required.' },
    });
    const input = getByLabelText('Service name');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(getByRole('alert')).toHaveTextContent('The name is required.');
  });

  it('sets `aria-invalid` to false when there is no error', () => {
    const { getByLabelText } = render(FormFieldHarness, {
      props: { id: 'svc-name', label: 'Service name' },
    });
    expect(getByLabelText('Service name').getAttribute('aria-invalid')).toBe('false');
  });
});
