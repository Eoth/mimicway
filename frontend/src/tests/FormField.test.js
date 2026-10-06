import { render } from '@testing-library/svelte';
import { describe, it, expect } from 'vitest';
import FormFieldHarness from './helpers/FormFieldHarness.svelte';

describe('FormField', () => {
  it('associe le label au champ via for/id', () => {
    const { getByLabelText } = render(FormFieldHarness, {
      props: { id: 'svc-name', label: 'Service name' },
    });
    expect(getByLabelText('Service name')).toBeInTheDocument();
  });

  it('relie le hint via aria-describedby', () => {
    const { getByLabelText, getByText } = render(FormFieldHarness, {
      props: { id: 'svc-name', label: 'Service name', hint: 'Unique identifier' },
    });
    const input = getByLabelText('Service name');
    const hint = getByText('Unique identifier');
    expect(input.getAttribute('aria-describedby')).toContain(hint.id);
  });

  it("affiche l'erreur avec role alert et aria-invalid sur le champ", () => {
    const { getByLabelText, getByRole } = render(FormFieldHarness, {
      props: { id: 'svc-name', label: 'Service name', error: 'The name is required.' },
    });
    const input = getByLabelText('Service name');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(getByRole('alert')).toHaveTextContent('The name is required.');
  });

  it("n'affiche pas aria-invalid quand il n'y a pas d'erreur", () => {
    const { getByLabelText } = render(FormFieldHarness, {
      props: { id: 'svc-name', label: 'Service name' },
    });
    expect(getByLabelText('Service name').getAttribute('aria-invalid')).toBe('false');
  });
});
