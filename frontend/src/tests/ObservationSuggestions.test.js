import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ObservationSuggestions from '../lib/components/ObservationSuggestions.svelte';
import {
  observeService,
  unobserveService,
  getObservationStatus,
  getServiceSuggestions,
  getLinkedBackends,
} from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  observeService: vi.fn(),
  unobserveService: vi.fn(),
  getObservationStatus: vi.fn(),
  getServiceSuggestions: vi.fn(),
  getLinkedBackends: vi.fn(),
}));

beforeEach(() => {
  vi.clearAllMocks();
  getObservationStatus.mockResolvedValue([]);
  getLinkedBackends.mockResolvedValue([]);
});

describe('ObservationSuggestions', () => {
  it('renders nothing and asks the server nothing for a mocked service', async () => {
    const { queryByTestId } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: true },
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(queryByTestId('observation-panel-svc-a')).not.toBeInTheDocument();
    expect(getObservationStatus).not.toHaveBeenCalled();
  });

  it('offers to observe a service not observed yet', async () => {
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Observe this service')).toBeInTheDocument());
  });

  it('shows a service already observed when it loads', async () => {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Stop observing')).toBeInTheDocument());
  });

  it('starts observing on click, with the service and its group', async () => {
    observeService.mockResolvedValue(undefined);
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', groupName: 'team-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Observe this service')).toBeInTheDocument());

    await fireEvent.click(getByText('Observe this service'));

    await waitFor(() => expect(getByText('Stop observing')).toBeInTheDocument());
    expect(observeService).toHaveBeenCalledWith('svc-a', 'team-a');
  });

  it('stops observing on click when already on', async () => {
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

  it('shows the error when observing fails', async () => {
    observeService.mockRejectedValue(new Error('a purely proxied service is required'));
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Observe this service')).toBeInTheDocument());

    await fireEvent.click(getByText('Observe this service'));

    await waitFor(() => expect(getByText('a purely proxied service is required')).toBeInTheDocument());
  });

  it('loads and shows an unconditional suggestion', async () => {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    getServiceSuggestions.mockResolvedValue([
      {
        method: 'GET',
        sub_path: 'orders/1',
        paths: ['orders/1'],
        path_count: 1,
        sample_count: 3,
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

  it('shows one card per rule of a conditional suggestion and passes its draft on click', async () => {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    getServiceSuggestions.mockResolvedValue([
      {
        method: 'GET',
        sub_path: 'orders',
        paths: ['orders'],
        path_count: 1,
        sample_count: 4,
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

  it('reports an unexplained variance with its call and response counts', async () => {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    getServiceSuggestions.mockResolvedValue([
      {
        method: 'GET',
        sub_path: '/flaky',
        paths: ['/flaky'],
        path_count: 1,
        sample_count: 4,
        outcome: 'VarianceUnexplained',
        response_class_count: 3,
      },
    ]);
    const { getByText } = render(ObservationSuggestions, {
      props: { serviceName: 'svc-a', isMocked: false },
    });
    await waitFor(() => expect(getByText('Stop observing')).toBeInTheDocument());

    await fireEvent.click(getByText('Refresh the suggestions'));

    await waitFor(() => expect(getByText(/^Varying responses observed/)).toBeInTheDocument());
    expect(getByText(/\(4 calls, 3 distinct responses\)/)).toBeInTheDocument();
  });

  // A suggestion for a route pattern, as the server sends it: the route fields next to the outcome.
  function routeSuggestion(fields) {
    return {
      method: 'GET',
      sub_path: '/service/{hash}/{name}',
      paths: ['/service/9f86d081/restBilling', '/service/9f86d081/restOrders', '/service/9f86d081/restStock'],
      path_count: 4,
      sample_count: 4,
      ...fields,
    };
  }

  async function showSuggestions(suggestions, props = {}) {
    getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
    getServiceSuggestions.mockResolvedValue(suggestions);
    const view = render(ObservationSuggestions, { props: { serviceName: 'svc-a', isMocked: false, ...props } });
    await waitFor(() => expect(view.getByText('Stop observing')).toBeInTheDocument());
    await fireEvent.click(view.getByText('Refresh the suggestions'));
    return view;
  }

  it('names the route a suggestion covers and a few of the paths it groups', async () => {
    const echo = {
      method: 'GET',
      sub_path: '/service/{hash}/{name}',
      condition: null,
      echo: { type: 'PathParam', key: 'name' },
      sample_count: 4,
      response: {
        status: 200,
        headers: [],
        body: [
          { type: 'Literal', value: '<url>http://backend/ws/' },
          { type: 'Template', template: '{{path.name}}' },
          { type: 'Literal', value: '</url>' },
        ],
        chaos: null,
      },
    };
    const onUseSuggestion = vi.fn();
    const { getByTestId, getByText } = await showSuggestions([routeSuggestion({ outcome: 'Echo', rule: echo })], {
      onUseSuggestion,
    });

    await waitFor(() =>
      expect(getByTestId('observation-suggestion-paths-svc-a-0')).toHaveTextContent(
        'GET /service/{hash}/{name} groups 4 observed paths, such as /service/9f86d081/restBilling, ' +
          '/service/9f86d081/restOrders, /service/9f86d081/restStock',
      ),
    );
    expect(getByText('no condition: the response repeats Path parameter "name" (4 calls)')).toBeInTheDocument();
    // The preview shows the expression that repeats the value, where it stands in the body.
    expect(getByText('<url>http://backend/ws/{{path.name}}</url>')).toBeInTheDocument();

    await fireEvent.click(getByText('Use this suggestion'));
    expect(onUseSuggestion).toHaveBeenCalledWith(
      expect.objectContaining({
        sub_path: '/service/{hash}/{name}',
        conditions: { all_of: [], any_of: [] },
        response: echo.response,
      }),
    );
  });

  it('names in plain words what a response repeats, and gives the form the script that writes its list', async () => {
    const rule = {
      method: 'POST',
      sub_path: '/ws/booking',
      conditions: {
        all_of: [{ source: { type: 'XPath', key: 'Envelope/Body/Book' }, operator: { type: 'Exists' } }],
        any_of: [],
      },
      condition: { source: { type: 'XPath', key: 'Envelope/Body/Book' }, operator: { type: 'Exists' } },
      echo: null,
      repeats: [
        { type: 'XPath', key: 'Envelope/Body/Book/ref' },
        { type: 'Header', key: 'x-channel' },
      ],
      list: { items: { type: 'XPath', key: 'Envelope/Body/Book/slot' }, fields: ['date', 'type'] },
      script: 'let items = parse_xml_items(request.body, "Envelope/Body/Book/slot");',
      sample_count: 3,
      response: {
        status: 200,
        headers: [],
        body: [
          { type: 'Literal', value: '<BookResponse>' },
          { type: 'Template', template: '{{script.items}}' },
          { type: 'Literal', value: '</BookResponse>' },
        ],
        chaos: null,
      },
    };
    const onUseSuggestion = vi.fn();
    const { getByText } = await showSuggestions(
      [routeSuggestion({ sub_path: '/ws/booking', operation: 'Book', outcome: 'Echo', rule })],
      { onUseSuggestion },
    );

    await waitFor(() =>
      expect(
        getByText(
          'no condition: the response repeats XML element of the body "ref" and HTTP header "x-channel"; ' +
            'the response writes one block per "slot" of the request, with its "date" and "type" (3 calls)',
        ),
      ).toBeInTheDocument(),
    );
    await fireEvent.click(getByText('Use this suggestion'));
    expect(onUseSuggestion).toHaveBeenCalledWith(
      expect.objectContaining({ script: rule.script, response_mode: 'advanced', response: rule.response }),
    );
  });

  it('names the one observed path a pattern stands for', async () => {
    const rule = {
      method: 'GET',
      sub_path: '/service/{hash}/status',
      condition: null,
      echo: null,
      sample_count: 3,
      response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'up' }], chaos: null },
    };
    const { getByTestId, queryByTestId } = await showSuggestions([
      routeSuggestion({
        sub_path: '/service/{hash}/status',
        paths: ['/service/9f86d081/status'],
        path_count: 1,
        outcome: 'Unconditional',
        rule,
      }),
      routeSuggestion({
        sub_path: '/health',
        paths: ['/health'],
        path_count: 1,
        outcome: 'Unconditional',
        rule: { ...rule, sub_path: '/health' },
      }),
    ]);

    await waitFor(() =>
      expect(getByTestId('observation-suggestion-paths-svc-a-0')).toHaveTextContent(
        'GET /service/{hash}/status stands for the observed path /service/9f86d081/status',
      ),
    );
    // A literal path needs no explanation.
    expect(queryByTestId('observation-suggestion-paths-svc-a-1')).not.toBeInTheDocument();
  });

  it('labels a condition on a path parameter', async () => {
    const rule = (value, status) => ({
      method: 'GET',
      sub_path: '/orders/{id}',
      condition: { source: { type: 'PathParam', key: 'id' }, operator: { type: 'Eq', value } },
      echo: null,
      sample_count: 1,
      response: { status, headers: [], body: [{ type: 'Literal', value: 'x' }], chaos: null },
    });
    const { getByText } = await showSuggestions([
      routeSuggestion({ sub_path: '/orders/{id}', outcome: 'Conditional', rules: [rule('1', 200), rule('2', 404)] }),
    ]);

    await waitFor(() => expect(getByText('if Path parameter "id" = "1"')).toBeInTheDocument());
    expect(getByText('if Path parameter "id" = "2"')).toBeInTheDocument();
  });

  it('says which route varies when no field explains its responses', async () => {
    const { getByTestId } = await showSuggestions([
      routeSuggestion({
        sub_path: '/status',
        paths: ['/status'],
        path_count: 1,
        outcome: 'VarianceUnexplained',
        response_class_count: 3,
      }),
    ]);

    await waitFor(() =>
      expect(getByTestId('observation-suggestion-unexplained-svc-a-0')).toHaveTextContent(/^GET \/status Varying/),
    );
  });

  describe('backends named in the responses', () => {
    async function refreshed(props) {
      getObservationStatus.mockResolvedValue([{ service_name: 'directory', group_name: 'team-a' }]);
      const view = render(ObservationSuggestions, {
        props: { serviceName: 'directory', groupName: 'team-a', isMocked: false, ...props },
      });
      await waitFor(() => expect(view.getByText('Refresh the suggestions')).toBeInTheDocument());
      await fireEvent.click(view.getByText('Refresh the suggestions'));
      return view;
    }

    it('lists each backend with the routes that name it, and who serves it', async () => {
      getServiceSuggestions.mockResolvedValue([]);
      getLinkedBackends.mockResolvedValue([
        {
          origin: 'http://orders.internal:8080',
          addresses: ['http://orders.internal:8080/ws/restOrders'],
          routes: ['GET /service/{name}'],
          served_by: [{ name: 'orders', group_name: 'team-a' }],
          draft: null,
        },
      ]);
      const { getByTestId, queryByTestId } = await refreshed();

      await waitFor(() => expect(getByTestId('observation-linked-backend-directory-0')).toBeInTheDocument());
      const item = getByTestId('observation-linked-backend-directory-0');
      expect(item).toHaveTextContent('http://orders.internal:8080');
      expect(item).toHaveTextContent('named by GET /service/{name}, such as http://orders.internal:8080/ws/restOrders');
      expect(item).toHaveTextContent('Served by the service orders.');
      expect(queryByTestId('observation-create-service-directory-0')).not.toBeInTheDocument();
      expect(getLinkedBackends).toHaveBeenCalledWith('directory', 'team-a');
    });

    it('opens the service form filled with a proxy in front of a backend no service reaches', async () => {
      getServiceSuggestions.mockResolvedValue([]);
      getLinkedBackends.mockResolvedValue([
        {
          origin: 'http://stock.internal',
          addresses: ['http://stock.internal/items'],
          routes: ['GET /'],
          served_by: [],
          draft: { name: 'stock', real_target_url: 'http://stock.internal', group_name: 'team-a' },
        },
      ]);
      const onCreateService = vi.fn();
      const { getByTestId } = await refreshed({ onCreateService });

      await waitFor(() => expect(getByTestId('observation-create-service-directory-0')).toBeInTheDocument());
      await fireEvent.click(getByTestId('observation-create-service-directory-0'));
      expect(onCreateService).toHaveBeenCalledWith({
        name: 'stock',
        listen_path: '',
        real_target_url: 'http://stock.internal',
        is_mocked: false,
        group_name: 'team-a',
        wsdl_mode: 'auto',
        rules: [],
      });
    });

    it('says which services a suggested rule sends the application to', async () => {
      getServiceSuggestions.mockResolvedValue([
        {
          method: 'GET',
          sub_path: '/',
          paths: ['/'],
          path_count: 1,
          sample_count: 3,
          outcome: 'Unconditional',
          rule: {
            method: 'GET',
            sub_path: '/',
            condition: null,
            sample_count: 3,
            redirected_to: [
              { name: 'orders', group_name: 'team-a' },
              { name: 'stock', group_name: 'team-a' },
            ],
            response: { status: 200, headers: [], body: [{ type: 'Literal', value: '{}' }] },
          },
        },
      ]);
      const { getByTestId } = await refreshed();

      await waitFor(() => expect(getByTestId('observation-suggestion-redirect-directory-0-0')).toBeInTheDocument());
      expect(getByTestId('observation-suggestion-redirect-directory-0-0')).toHaveTextContent(
        'Sends the application to the services orders, stock in Mimicway, instead of the backends behind them.',
      );
    });
  });

  describe('rules with several values, operations and calls left out', () => {
    const eq = (type, key, value) => ({ source: { type, key }, operator: { type: 'Eq', value } });
    const response = (value) => ({ status: 200, headers: [], body: [{ type: 'Literal', value }], chaos: null });

    async function shown(suggestions) {
      getObservationStatus.mockResolvedValue([{ service_name: 'svc-a', group_name: null }]);
      getServiceSuggestions.mockResolvedValue(suggestions);
      const onUseSuggestion = vi.fn();
      const view = render(ObservationSuggestions, {
        props: { serviceName: 'svc-a', isMocked: false, onUseSuggestion },
      });
      await waitFor(() => expect(view.getByText('Stop observing')).toBeInTheDocument());
      await fireEvent.click(view.getByText('Refresh the suggestions'));
      return { ...view, onUseSuggestion };
    }

    it('says the values a rule accepts, and passes all of them to the rule form', async () => {
      const both = [eq('PathParam', 'name', 'orders'), eq('PathParam', 'name', 'ordersV2')];
      const { getByText, getAllByText, getByTestId, onUseSuggestion } = await shown([
        {
          method: 'GET',
          sub_path: '/directory/{name}',
          paths: ['/directory/orders', '/directory/ordersV2', '/directory/stock'],
          path_count: 3,
          sample_count: 6,
          operation: null,
          left_out: [{ status: 500, count: 1 }],
          truncated_request_bodies: 0,
          outcome: 'Conditional',
          rules: [
            {
              method: 'GET',
              sub_path: '/directory/{name}',
              conditions: { all_of: [], any_of: both },
              condition: null,
              sample_count: 4,
              response: response('<endpoint>/ws/OrderService</endpoint>'),
            },
            {
              method: 'GET',
              sub_path: '/directory/{name}',
              conditions: { all_of: [eq('PathParam', 'name', 'stock')], any_of: [] },
              condition: eq('PathParam', 'name', 'stock'),
              sample_count: 1,
              response: response('<endpoint>/ws/StockService</endpoint>'),
            },
          ],
        },
      ]);

      await waitFor(() => expect(getByText('if Path parameter "name" is "orders" or "ordersV2"')).toBeInTheDocument());
      expect(getByText('if Path parameter "name" = "stock"')).toBeInTheDocument();
      expect(getByTestId('observation-suggestion-left-out-svc-a-0')).toHaveTextContent(
        '1 call was left out: it got a server error (500) that nothing in the request explains.',
      );

      await fireEvent.click(getAllByText('Use this suggestion')[0]);
      expect(onUseSuggestion).toHaveBeenCalledWith(
        expect.objectContaining({ conditions: { all_of: [], any_of: both } }),
      );
    });

    it('names the operation of a suggestion once, and the field that decides on each rule', async () => {
      const operation = {
        source: { type: 'XPath', key: 'Envelope/Body/GetStatus' },
        operator: { type: 'Exists', value: undefined },
      };
      const reference = eq('XPath', 'Envelope/Body/GetStatus/ref', 'R1');
      const { getByText, getByTestId, queryByText, onUseSuggestion } = await shown([
        {
          method: 'POST',
          sub_path: '/ws/OrderService',
          paths: ['/ws/OrderService'],
          path_count: 1,
          sample_count: 4,
          operation: 'GetStatus',
          left_out: [],
          truncated_request_bodies: 2,
          non_utf8_request_bodies: 1,
          outcome: 'Conditional',
          rules: [
            {
              method: 'POST',
              sub_path: '/ws/OrderService',
              conditions: { all_of: [operation, reference], any_of: [] },
              condition: null,
              sample_count: 4,
              response: response('<status>R1-OPEN</status>'),
            },
          ],
        },
      ]);

      await waitFor(() =>
        expect(getByTestId('observation-suggestion-operation-svc-a-0')).toHaveTextContent(
          'SOAP operation GetStatus: its rules answer only the calls that name it.',
        ),
      );
      expect(getByText('if XML element of the body "Envelope/Body/GetStatus/ref" = "R1"')).toBeInTheDocument();
      expect(queryByText(/Envelope\/Body\/GetStatus"/)).not.toBeInTheDocument();
      expect(getByTestId('observation-suggestion-truncated-svc-a-0')).toHaveTextContent(
        '2 request bodies were cut at the size limit of observation: their fields could not be read.',
      );
      expect(getByTestId('observation-suggestion-non-utf8-svc-a-0')).toHaveTextContent(
        '1 request body is in another encoding than UTF-8: its fields could not be read, and no rule can read them.',
      );

      await fireEvent.click(getByText('Use this suggestion'));
      expect(onUseSuggestion).toHaveBeenCalledWith(
        expect.objectContaining({ conditions: { all_of: [operation, reference], any_of: [] } }),
      );
    });

    it('says an unconditional rule of an operation has no other condition', async () => {
      const operation = { source: { type: 'XPath', key: 'Envelope/Body/Ping' }, operator: { type: 'Exists' } };
      const { getByText } = await shown([
        {
          method: 'POST',
          sub_path: '/ws/OrderService',
          paths: ['/ws/OrderService'],
          path_count: 1,
          sample_count: 3,
          operation: 'Ping',
          left_out: [],
          truncated_request_bodies: 0,
          outcome: 'Unconditional',
          rule: {
            method: 'POST',
            sub_path: '/ws/OrderService',
            conditions: { all_of: [operation], any_of: [] },
            condition: operation,
            sample_count: 3,
            response: response('<Pong/>'),
          },
        },
      ]);
      await waitFor(() => expect(getByText('no condition (3 identical calls)')).toBeInTheDocument());
    });
  });
});
