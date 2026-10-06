import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import ServiceDetail from '../lib/components/ServiceDetail.svelte';
import { updateService, deleteService, reorderRules } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  updateService: vi.fn(),
  deleteService: vi.fn(),
  reorderRules: vi.fn(),
}));

function svc(overrides = {}) {
  return {
    name: 'svc-a',
    group_name: null,
    listen_path: '/v1/*',
    real_target_url: 'http://backend:8080',
    is_mocked: true,
    rewrite_directory_urls: false,
    rules: [],
    ...overrides,
  };
}

describe('ServiceDetail - suppression (regression crash null)', () => {
  it('ne remonte pas de fausse erreur "Cannot read properties of null" si le service devient null pendant la suppression en vol', async () => {
    let resolveDelete;
    deleteService.mockReturnValue(
      new Promise((resolve) => {
        resolveDelete = resolve;
      }),
    );
    const onNotify = vi.fn();
    const onDelete = vi.fn();

    const { getByText, rerender } = render(ServiceDetail, {
      props: { service: svc(), onNotify, onDelete },
    });

    await fireEvent.click(getByText('Delete'));
    await fireEvent.click(getByText('Yes, delete'));

    // Simule la course reactive : le parent (App.svelte) a deja retire le
    // service courant (prop devient null) AVANT que la promesse de
    // suppression ne se resolve — c'est exactement le scenario reproduit
    // manuellement qui causait "Cannot read properties of null (reading
    // 'name')" avant le correctif (capture de name/groupName avant l'await).
    await rerender({ service: null, onNotify, onDelete });

    resolveDelete();
    await waitFor(() => expect(onNotify).toHaveBeenCalled());

    expect(onNotify).toHaveBeenCalledWith(expect.stringContaining('svc-a'), 'success');
    expect(onNotify).not.toHaveBeenCalledWith(expect.stringContaining('Cannot read'), 'error');
    expect(onDelete).toHaveBeenCalledWith('svc-a', null);
  });

  it('supprime via la route scopee au groupe quand le service appartient a un groupe', async () => {
    deleteService.mockResolvedValue(null);
    const onNotify = vi.fn();
    const onDelete = vi.fn();

    const { getByText } = render(ServiceDetail, {
      props: { service: svc({ group_name: 'team-a' }), onNotify, onDelete },
    });

    await fireEvent.click(getByText('Delete'));
    await fireEvent.click(getByText('Yes, delete'));

    await waitFor(() => expect(deleteService).toHaveBeenCalledWith('svc-a', 'team-a'));
    expect(onDelete).toHaveBeenCalledWith('svc-a', 'team-a');
  });
});

describe('ServiceDetail - formulaire d edition (regression URL sans code de groupe)', () => {
  it('transmet availableGroups au formulaire d edition pour que l URL de test inclue le code du groupe', async () => {
    const { getByText } = render(ServiceDetail, {
      props: {
        service: svc({ group_name: 'team-a' }),
        availableGroups: [{ name: 'team-a', code: 'ab3f9' }],
      },
    });

    await fireEvent.click(getByText('Edit the service'));

    await waitFor(() => expect(getByText(/\/ab3f9\/svc-a\/v1\/\*/)).toBeInTheDocument());
  });
});

describe('ServiceDetail - test URL', () => {
  it('shows the URL to call, with the code of the group the service belongs to', () => {
    const { getByText } = render(ServiceDetail, {
      props: { service: svc({ group_name: 'team-a' }), availableGroups: [{ name: 'team-a', code: 'ab3f9' }] },
    });
    expect(getByText(`${window.location.origin}/ab3f9/svc-a/v1/*`)).toBeInTheDocument();
  });

  it('shows the URL of an ungrouped service without any prefix', () => {
    const { getByText } = render(ServiceDetail, { props: { service: svc() } });
    expect(getByText(`${window.location.origin}/svc-a/v1/*`)).toBeInTheDocument();
  });
});
