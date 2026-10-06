import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ObservationSuggestions from '../lib/components/ObservationSuggestions.svelte';
import { observeService, unobserveService, getObservationStatus, getServiceSuggestions } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  observeService: vi.fn(),
  unobserveService: vi.fn(),
  getObservationStatus: vi.fn(),
  getServiceSuggestions: vi.fn(),
}));

beforeEach(() => {
  vi.clearAllMocks();
  getObservationStatus.mockResolvedValue([]);
});

describe('ObservationSuggestions', () => {
  it('ne rend rien pour un service mocke (is_mocked=true)', async () => {
    const { queryByTestId } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: true },
    });
    await waitFor(() => {});
    expect(queryByTestId('observation-panel-svc-a')).not.toBeInTheDocument();
  });

  it('affiche "Observer ce service" quand le service n\'est pas encore observe', async () => {
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Observe this service')).toBeInTheDocument());
  });

  it('reflete un service deja observe au chargement', async () => {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Stop observing')).toBeInTheDocument());
  });

  it("active l'observation au clic et appelle observeService avec le bon service/groupe", async () => {
    observeService.mockResolvedValue(undefined);
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', groupName: 'team-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Observe this service')).toBeInTheDocument());

    await fireEvent.click(getByText('Observe this service'));

    await waitFor(() => expect(getByText('Stop observing')).toBeInTheDocument());
    expect(observeService).toHaveBeenCalledWith('svc-a', 'team-a');
  });

  it("desactive l'observation au clic quand deja active", async () => {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    unobserveService.mockResolvedValue(undefined);
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Stop observing')).toBeInTheDocument());

    await fireEvent.click(getByText('Stop observing'));

    await waitFor(() => expect(getByText('Observe this service')).toBeInTheDocument());
    expect(unobserveService).toHaveBeenCalledWith('svc-a', null);
  });

  it("affiche une erreur si l'activation echoue", async () => {
    observeService.mockRejectedValue(new Error('a purely proxied service is required'));
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Observe this service')).toBeInTheDocument());

    await fireEvent.click(getByText('Observe this service'));

    await waitFor(() => expect(getByText('a purely proxied service is required')).toBeInTheDocument());
  });

  it('charge et affiche une suggestion inconditionnelle', async () => {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    getServiceSuggestions.mockResolvedValue([
      {
        outcome: 'Unconditional',
        rule: {
          method: 'GET',
          sub_path: 'orders/1',
          condition: null,
          sample_count: 3,
          response: { status: 200, headers: [], body: [{ type: 'Literal', value: '{"ok":true}' }], chaos: null },
        },
      },
    ]);
    const { getByText, getByTestId } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Stop observing')).toBeInTheDocument());

    await fireEvent.click(getByText('Refresh the suggestions'));

    await waitFor(() => expect(getByText('GET orders/1')).toBeInTheDocument());
    expect(getByText('no condition (3 identical calls)')).toBeInTheDocument();
    expect(getByTestId('observation-suggestion-svc-a-0-0')).toBeInTheDocument();
  });

  it('affiche une suggestion conditionnelle avec une carte par regle et transmet le brouillon au clic', async () => {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    getServiceSuggestions.mockResolvedValue([
      {
        outcome: 'Conditional',
        rules: [
          {
            method: 'GET',
            sub_path: 'orders',
            condition: { source: { type: 'QueryParam', key: 'id' }, operator: { type: 'Eq', value: '1' } },
            sample_count: 2,
            response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'found' }], chaos: null },
          },
          {
            method: 'GET',
            sub_path: 'orders',
            condition: { source: { type: 'QueryParam', key: 'id' }, operator: { type: 'Eq', value: '2' } },
            sample_count: 2,
            response: { status: 404, headers: [], body: [{ type: 'Literal', value: 'missing' }], chaos: null },
          },
        ],
      },
    ]);
    const onUseSuggestion = vi.fn();
    const { getByText, getAllByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false, onUseSuggestion },
    });
    await waitFor(() => expect(getByText('Stop observing')).toBeInTheDocument());

    await fireEvent.click(getByText('Refresh the suggestions'));

    await waitFor(() => expect(getByText('if Query parameter "id" = "1"')).toBeInTheDocument());
    expect(getByText('if Query parameter "id" = "2"')).toBeInTheDocument();

    const useButtons = getAllByText('Use this suggestion');
    expect(useButtons).toHaveLength(2);
    await fireEvent.click(useButtons[0]);

    expect(onUseSuggestion).toHaveBeenCalledWith(
      expect.objectContaining({
        name: '',
        method: 'GET',
        sub_path: 'orders',
        action: 'mock',
        conditions: {
          all_of: [{ source: { type: 'QueryParam', key: 'id' }, operator: { type: 'Eq', value: '1' } }],
          any_of: [],
        },
        response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'found' }], chaos: null },
      }),
    );
  });

  it('affiche un signal sans regle proposable pour une variance inexpliquee', async () => {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    getServiceSuggestions.mockResolvedValue([
      { outcome: 'VarianceUnexplained', sample_count: 4, response_class_count: 3 },
    ]);
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Stop observing')).toBeInTheDocument());

    await fireEvent.click(getByText('Refresh the suggestions'));

    await waitFor(() => expect(getByText(/^Varying responses observed/)).toBeInTheDocument());
    expect(getByText(/\(4 calls, 3 distinct responses\)/)).toBeInTheDocument();
  });
});
