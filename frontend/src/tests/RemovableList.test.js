import { render, fireEvent } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RemovableList from '../lib/components/RemovableList.svelte';

describe('RemovableList', () => {
  it('shows the empty text when there are no items', () => {
    const { getByText } = render(RemovableList, { props: { items: [], emptyText: 'Nothing here.' } });
    expect(getByText('Nothing here.')).toBeInTheDocument();
  });

  it('shows each item with a remove button', () => {
    const { getByText, getByLabelText } = render(RemovableList, {
      props: { items: ['alice', 'bob'] },
    });
    expect(getByText('alice')).toBeInTheDocument();
    expect(getByText('bob')).toBeInTheDocument();
    expect(getByLabelText('Remove alice')).toBeInTheDocument();
    expect(getByLabelText('Remove bob')).toBeInTheDocument();
  });

  it('calls `onRemove` with the matching item', async () => {
    const onRemove = vi.fn();
    const { getByLabelText } = render(RemovableList, {
      props: { items: ['alice', 'bob'], onRemove },
    });
    await fireEvent.click(getByLabelText('Remove bob'));
    expect(onRemove).toHaveBeenCalledWith('bob');
  });

  it('takes object items through `getKey` and `getLabel`', () => {
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
