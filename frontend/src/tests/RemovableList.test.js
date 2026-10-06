import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RemovableList from '../lib/components/RemovableList.svelte';

describe('RemovableList', () => {
  it("affiche le texte vide quand il n'y a pas d'items", () => {
    const { getByText } = render(RemovableList, { props: { items: [], emptyText: 'Nothing here.' } });
    expect(getByText('Nothing here.')).toBeInTheDocument();
  });

  it('affiche chaque item avec un bouton de suppression', () => {
    const { getByText, getByLabelText } = render(RemovableList, {
      props: { items: ['alice', 'bob'] },
    });
    expect(getByText('alice')).toBeInTheDocument();
    expect(getByText('bob')).toBeInTheDocument();
    expect(getByLabelText('Remove alice')).toBeInTheDocument();
    expect(getByLabelText('Remove bob')).toBeInTheDocument();
  });

  it("appelle onRemove avec l'item correspondant", async () => {
    const onRemove = vi.fn();
    const { getByLabelText } = render(RemovableList, {
      props: { items: ['alice', 'bob'], onRemove },
    });
    await fireEvent.click(getByLabelText('Remove bob'));
    expect(onRemove).toHaveBeenCalledWith('bob');
  });

  it('supporte des items objets via getKey/getLabel', () => {
    const items = [{ username: 'alice' }, { username: 'bob' }];
    const { getByText } = render(RemovableList, {
      props: {
        items,
        getKey: (i) => i.username,
        getLabel: (i) => i.username,
      },
    });
    expect(getByText('alice')).toBeInTheDocument();
    expect(getByText('bob')).toBeInTheDocument();
  });
});
