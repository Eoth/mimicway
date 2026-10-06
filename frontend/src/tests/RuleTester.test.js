import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RuleTester from '../lib/components/RuleTester.svelte';
import { testRule } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  testRule: vi.fn(),
}));

const logWithDetail = {
  timestamp: new Date('2026-01-10T12:00:00').getTime(),
  service_name: 'svc-a',
  method: 'GET',
  path: '/svc-a/orders/42',
  mode: 'mock',
  rule_matched: 'r1',
  target_url: null,
  status: 200,
  captured: {
    remaining_path: '/orders/42',
    path_params: {},
    query_params: { foo: 'bar' },
    headers: { 'x-env': 'prod' },
    body: '',
    body_truncated: false,
    content_type: null,
  },
};

const logWithoutDetail = {
  timestamp: new Date('2026-01-10T12:05:00').getTime(),
  service_name: 'svc-a',
  method: 'GET',
  path: '/svc-a/passthrough',
  mode: 'proxy',
  rule_matched: null,
  target_url: 'http://backend/passthrough',
  status: 200,
  captured: null,
};

function draft(overrides = {}) {
  return () => ({
    method: 'GET',
    subPath: '',
    allOf: [],
    anyOf: [],
    ...overrides,
  });
}

describe('RuleTester: filtrage des entrees sans detail', () => {
  it("affiche un message quand aucune requete n'a ete capturee", () => {
    const { getByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [], getDraftRule: draft() },
    });
    expect(getByText('No request has been captured for this service yet.')).toBeInTheDocument();
  });

  it('affiche un message quand seules des entrees sans detail existent', () => {
    const { getByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithoutDetail], getDraftRule: draft() },
    });
    expect(getByText(/requests proxied directly/)).toBeInTheDocument();
  });

  it('ne liste que les entrees avec detail capture dans le selecteur', () => {
    const { getByLabelText, queryByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail, logWithoutDetail], getDraftRule: draft() },
    });
    const select = getByLabelText('Captured request');
    const options = [...select.querySelectorAll('option')].filter((o) => o.value !== '');
    expect(options).toHaveLength(1);
    expect(queryByText(/passthrough/)).not.toBeInTheDocument();
  });
});

describe('RuleTester: appel API et affichage du resultat', () => {
  it('envoie le brouillon de regle et la requete capturee choisie', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
    });

    const { getByLabelText, getByRole } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({
          allOf: [{ source: { type: 'QueryParam', key: 'foo' }, operator: { type: 'Eq', value: 'bar' } }],
        }),
      },
    });

    const select = getByLabelText('Captured request');
    await fireEvent.change(select, { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(testRule).toHaveBeenCalled());
    const [payload] = testRule.mock.calls[0];
    expect(payload.method).toBe('GET');
    expect(payload.conditions.all_of[0].source.key).toBe('foo');
    expect(payload.request.query_params).toEqual({ foo: 'bar' });
    expect(payload.request.remaining_path).toBe('/orders/42');
  });

  it("envoie l'action et les 3 blocs de script du brouillon (pas seulement le matching)", async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
    });

    const { getByLabelText, getByRole } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({
          action: 'mock',
          preScript: 'let x = 1;',
          script: '"hello"',
          postScript: null,
        }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(testRule).toHaveBeenCalled());
    // .at(-1) plutot que mock.calls[0] : testRule est un mock PARTAGE (pas
    // reinitialise entre les `it()` de ce fichier), donc [0] renverrait
    // l'appel du tout premier test du describe plutot que celui de ce test.
    const [payload] = testRule.mock.calls.at(-1);
    expect(payload.action).toBe('mock');
    expect(payload.pre_script).toBe('let x = 1;');
    expect(payload.script).toBe('"hello"');
    expect(payload.post_script).toBeNull();
  });

  it('affiche le detail condition par condition avec icone et texte (pas seulement de la couleur)', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: false,
      body_truncated: false,
      all_of: [
        {
          condition: { source: { type: 'QueryParam', key: 'foo' }, operator: { type: 'Eq', value: 'bar' } },
          matched: false,
          found_value: null,
          hint: "'foo' was not found as a query parameter, but it is present as a path parameter in this request",
        },
      ],
      any_of: [],
    });

    const { getByLabelText, getByRole, getByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail], getDraftRule: draft() },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByText('✗ This rule would not match this request')).toBeInTheDocument());
    expect(getByText(/: does not match \(/)).toBeInTheDocument();
    expect(getByText(/\(value found: none\)/)).toBeInTheDocument();
    expect(getByText(/present as a path parameter/)).toBeInTheDocument();
  });

  it("affiche une banniere d'avertissement quand le corps capture est tronque", async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: false,
      body_truncated: true,
      all_of: [
        {
          condition: { source: { type: 'JsonPointer', key: '/a' }, operator: { type: 'Exists' } },
          matched: false,
          found_value: null,
          hint: null,
        },
      ],
      any_of: [],
    });

    const { getByLabelText, getByRole, getByText } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail], getDraftRule: draft() },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() =>
      expect(
        getByText('⚠ The body of this request was truncated in the log: comparisons on the body may be wrong.'),
      ).toBeInTheDocument(),
    );
  });

  it("affiche un message clair quand un script echoue a l'execution", async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [
        { slot: 'script', message: 'Function not found: totally_undefined_fn (i64, i64) (line 1, position 1)' },
      ],
    });

    const { getByLabelText, getByRole, getByTestId, getByText } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: 'totally_undefined_fn(1, 2)' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByTestId('rule-tester-script-errors')).toBeInTheDocument());
    expect(getByText(/^⚠ A script failed to run:/)).toBeInTheDocument();
    expect(getByTestId('rule-tester-script-error-script')).toBeInTheDocument();
    expect(getByText(/totally_undefined_fn/)).toBeInTheDocument();
  });

  it("n'affiche aucune banniere d'erreur de script quand tous les scripts reussissent", async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
    });

    const { getByLabelText, getByRole, queryByTestId } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: '"hello"' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(queryByTestId('rule-tester-result')).toBeInTheDocument());
    expect(queryByTestId('rule-tester-script-errors')).not.toBeInTheDocument();
  });

  // --- script_results : visibilite d'un resultat REUSSI mais errone. Un
  // script sans erreur d'execution peut quand meme produire un resultat
  // inattendu (typo de cle, objet imbrique non navigable) — ces tests
  // verifient que le testeur montre desormais CE QUE le script a reellement
  // produit, pas seulement l'absence d'erreur.

  it('affiche les champs produits par un script reussi (ex: objet pioche via seeded_pick)', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
      script_results: [{ slot: 'script', value: '', fields: { name: 'Lyon', cp: '69000', insee: '69123' } }],
    });

    const { getByLabelText, getByRole, getByTestId, getByText } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: 'seeded_pick(request.path.siret, villes)' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByTestId('rule-tester-script-results')).toBeInTheDocument());
    expect(getByTestId('rule-tester-script-result-script')).toBeInTheDocument();
    expect(getByText('{{script.name}}')).toBeInTheDocument();
    expect(getByText('Lyon')).toBeInTheDocument();
    expect(getByText('{{script.cp}}')).toBeInTheDocument();
    expect(getByText('69000')).toBeInTheDocument();
  });

  it("affiche la valeur JSON reelle d'un champ imbrique (pas la syntaxe Rhai #{...})", async () => {
    // Cas precis diagnostique : un objet pioche imbrique sous une cle
    // ("ville") reste une seule cle plate cote {{script.champ}} — le
    // testeur doit exposer ca clairement, ce qui permet a l'utilisateur de
    // constater qu'un chemin imbrique {{script.ville.name}} n'existe pas.
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
      script_results: [
        {
          slot: 'script',
          value: '',
          fields: { ville: '{"cp":"69000","insee":"69123","name":"Lyon"}', id: 'fixed-id' },
        },
      ],
    });

    const { getByLabelText, getByRole, getByTestId, getByText } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: 'let ville = seeded_pick(...); #{ ville: ville, id: "fixed-id" }' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByTestId('rule-tester-script-results')).toBeInTheDocument());
    expect(getByText('{{script.ville}}')).toBeInTheDocument();
    expect(getByText('{"cp":"69000","insee":"69123","name":"Lyon"}')).toBeInTheDocument();
  });

  it('affiche la valeur simple ({{slot}}) quand le script retourne un scalaire (pas un map)', async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
      script_results: [{ slot: 'script', value: 'hello', fields: {} }],
    });

    const { getByLabelText, getByRole, getByTestId, getByText } = render(RuleTester, {
      props: {
        serviceName: 'svc-a',
        logs: [logWithDetail],
        getDraftRule: draft({ script: '"hello"' }),
      },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(getByTestId('rule-tester-script-results')).toBeInTheDocument());
    expect(getByText('{{script}}')).toBeInTheDocument();
    expect(getByText('hello')).toBeInTheDocument();
  });

  it("n'affiche aucun panneau de resultat de script quand aucun script n'est configure", async () => {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [],
      script_results: [],
    });

    const { getByLabelText, getByRole, queryByTestId } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail], getDraftRule: draft() },
    });

    await fireEvent.change(getByLabelText('Captured request'), { target: { value: '0' } });
    await fireEvent.click(getByRole('button', { name: /Test against this request/ }));

    await waitFor(() => expect(queryByTestId('rule-tester-result')).toBeInTheDocument());
    expect(queryByTestId('rule-tester-script-results')).not.toBeInTheDocument();
  });
});

describe('RuleTester: script errors', () => {
  async function scriptErrorText() {
    testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [{ slot: 'script', message: 'boom' }],
    });
    const { container, getByTestId } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [logWithDetail], getDraftRule: draft({ script: 'boom()' }) },
    });
    await fireEvent.change(container.querySelector('[data-testid="rule-tester-log-select"]'), {
      target: { value: '0' },
    });
    await fireEvent.click(container.querySelector('[data-testid="rule-tester-test-button"]'));
    await waitFor(() => expect(getByTestId('rule-tester-script-error-script')).toBeInTheDocument());
    return getByTestId('rule-tester-script-error-script').textContent.trim();
  }

  it('punctuates the slot and its error as English does', async () => {
    expect(await scriptErrorText()).toBe('Custom script: boom');
  });
});
