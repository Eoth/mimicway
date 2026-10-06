import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { describe, it, expect, vi } from 'vitest';
import RuleForm from '../lib/components/RuleForm.svelte';
import { getLogs, checkRuleConflicts } from '../lib/api.js';

vi.mock('../lib/api.js', () => ({
  validateScript: vi.fn(),
  getLogs: vi.fn(),
  checkRuleConflicts: vi.fn(),
}));

async function setInput(el, value) {
  el.value = value;
  await fireEvent.input(el);
}

async function submitForm(container) {
  const form = container.querySelector('form');
  await fireEvent.submit(form);
}

describe('RuleForm: rule name uniqueness', () => {
  it('refuse un nom de regle deja existant dans le service', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(RuleForm, {
      props: { existingRules: [{ name: 'existing-rule' }], onSave },
    });

    await setInput(getByLabelText('Rule name'), 'existing-rule');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('A rule named "existing-rule" already exists in this service.');
  });

  it('refuse un doublon insensible a la casse', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(RuleForm, {
      props: { existingRules: [{ name: 'My-Rule' }], onSave },
    });

    await setInput(getByLabelText('Rule name'), 'my-rule');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('A rule named "my-rule" already exists in this service.');
  });

  it('accepte un nom unique', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, container } = render(RuleForm, {
      props: { existingRules: [{ name: 'other-rule' }], onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalled());
  });

  it('accepte le meme nom en edition (exclus de la liste)', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const existingRule = {
      name: 'edit-me',
      action: 'mock',
      conditions: { all_of: [], any_of: [] },
      response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'ok' }], chaos: null },
    };
    const { container } = render(RuleForm, {
      props: { rule: existingRule, existingRules: [], onSave },
    });

    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalled());
  });

  it('refuse un nom de regle vide', async () => {
    const onSave = vi.fn();
    const { getByLabelText, container, getByRole } = render(RuleForm, {
      props: { onSave },
    });

    await setInput(getByLabelText('Rule name'), '');
    await submitForm(container);
    expect(onSave).not.toHaveBeenCalled();
    expect(getByRole('alert')).toHaveTextContent('The rule name is required.');
  });
});

describe('RuleForm: pre_script / post_script', () => {
  it('envoie pre_script et post_script a null quand les toggles restent desactives', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, container } = render(RuleForm, { props: { onSave } });

    await setInput(getByLabelText('Rule name'), 'r1');
    await submitForm(container);

    await waitFor(() => expect(onSave).toHaveBeenCalled());
    const [payload] = onSave.mock.calls[0];
    expect(payload.pre_script).toBeNull();
    expect(payload.post_script).toBeNull();
  });

  it('les toggles Pré-script et Post-script sont replies par defaut derriere "Options avancées"', () => {
    const { getByRole, queryByRole } = render(RuleForm);
    expect(getByRole('button', { name: /Advanced options/, expanded: false })).toBeInTheDocument();
    expect(queryByRole('switch', { name: 'Pre-script (preparation)' })).not.toBeInTheDocument();
    expect(queryByRole('switch', { name: 'Post-script (finalization)' })).not.toBeInTheDocument();
  });

  it('deplier "Options avancées" affiche les toggles Pré-script et Post-script', async () => {
    const { getByRole } = render(RuleForm);
    await fireEvent.click(getByRole('button', { name: /Advanced options/ }));
    expect(getByRole('switch', { name: 'Pre-script (preparation)' })).toBeInTheDocument();
    expect(getByRole('switch', { name: 'Post-script (finalization)' })).toBeInTheDocument();
  });

  it('inclut pre_script et post_script dans le payload une fois actives et remplis', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, getByRole, container } = render(RuleForm, { props: { onSave } });

    await setInput(getByLabelText('Rule name'), 'r2');
    await fireEvent.click(getByRole('button', { name: /Advanced options/ }));
    await fireEvent.click(getByRole('switch', { name: 'Pre-script (preparation)' }));
    await fireEvent.click(getByRole('switch', { name: 'Post-script (finalization)' }));

    const preTextarea = container.querySelector('#rule-pre-script');
    const postTextarea = container.querySelector('#rule-post-script');
    expect(preTextarea).toBeInTheDocument();
    expect(postTextarea).toBeInTheDocument();

    await setInput(preTextarea, '"pre-result"');
    await setInput(postTextarea, '"post-result"');
    await submitForm(container);

    await waitFor(() => expect(onSave).toHaveBeenCalled());
    const [payload] = onSave.mock.calls[0];
    expect(payload.pre_script).toBe('"pre-result"');
    expect(payload.post_script).toBe('"post-result"');
  });

  it("n'envoie pas pre_script si le champ reste vide meme toggle actif", async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, getByRole, container } = render(RuleForm, { props: { onSave } });

    await setInput(getByLabelText('Rule name'), 'r3');
    await fireEvent.click(getByRole('button', { name: /Advanced options/ }));
    await fireEvent.click(getByRole('switch', { name: 'Pre-script (preparation)' }));
    await submitForm(container);

    await waitFor(() => expect(onSave).toHaveBeenCalled());
    const [payload] = onSave.mock.calls[0];
    expect(payload.pre_script).toBeNull();
  });
});

describe('RuleForm: ouverture automatique des "Options avancées"', () => {
  const baseRule = {
    name: 'existing',
    action: 'mock',
    conditions: { all_of: [], any_of: [] },
    response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'ok' }], chaos: null },
  };

  it("reste repliee a l'ouverture d'une regle sans pre_script ni post_script", () => {
    const { getByRole, queryByRole } = render(RuleForm, { props: { rule: baseRule } });
    expect(getByRole('button', { name: /Advanced options/, expanded: false })).toBeInTheDocument();
    expect(queryByRole('switch', { name: 'Pre-script (preparation)' })).not.toBeInTheDocument();
  });

  it("s'ouvre automatiquement si post_script a deja du contenu", () => {
    const rule = { ...baseRule, post_script: '"already set"' };
    const { getByRole } = render(RuleForm, { props: { rule } });
    expect(getByRole('button', { name: /Advanced options/, expanded: true })).toBeInTheDocument();
    expect(getByRole('switch', { name: 'Post-script (finalization)' })).toBeInTheDocument();
  });

  it("s'ouvre automatiquement si pre_script a deja du contenu", () => {
    const rule = { ...baseRule, pre_script: '"already set"' };
    const { getByRole } = render(RuleForm, { props: { rule } });
    expect(getByRole('button', { name: /Advanced options/, expanded: true })).toBeInTheDocument();
    expect(getByRole('switch', { name: 'Pre-script (preparation)' })).toBeInTheDocument();
  });

  it('replier/deplier "Options avancées" ne fait perdre aucun contenu deja saisi', async () => {
    const { getByRole, container } = render(RuleForm);

    await fireEvent.click(getByRole('button', { name: /Advanced options/ }));
    await fireEvent.click(getByRole('switch', { name: 'Pre-script (preparation)' }));
    const preTextarea = container.querySelector('#rule-pre-script');
    await setInput(preTextarea, '"contenu saisi"');

    // Replier la zone : le contenu ne doit pas etre reinitialise (pas de
    // demontage du composant, juste un attribut `hidden`).
    await fireEvent.click(getByRole('button', { name: /Advanced options/ }));
    expect(container.querySelector('#rule-pre-script').value).toBe('"contenu saisi"');

    await fireEvent.click(getByRole('button', { name: /Advanced options/ }));
    expect(getByRole('switch', { name: 'Pre-script (preparation)' })).toBeInTheDocument();
    expect(container.querySelector('#rule-pre-script')).toBeVisible();
    expect(container.querySelector('#rule-pre-script').value).toBe('"contenu saisi"');
  });
});

describe('RuleForm: assistance de saisie path/query param', () => {
  it('ne rend pas le testeur de regle sans serviceName (retrocompat)', () => {
    const { queryByText } = render(RuleForm);
    expect(queryByText('Test against a real request')).not.toBeInTheDocument();
  });

  it('rend le testeur de regle quand serviceName est fourni', async () => {
    getLogs.mockResolvedValue([]);
    const { getByText } = render(RuleForm, { props: { serviceName: 'svc-a' } });
    await waitFor(() => expect(getByText('Test against a real request')).toBeInTheDocument());
  });

  it('combine les path params du service et du sous-chemin de la regle', async () => {
    getLogs.mockResolvedValue([]);
    const { getByLabelText, getByRole } = render(RuleForm, {
      props: { serviceName: 'svc-a', listenPath: '/orders/{id}' },
    });
    await setInput(getByLabelText('Sub-path (optional)'), '/items/{itemId}');
    await fireEvent.click(getByRole('button', { name: '+ AND condition' }));
    const sourceSelect = getByLabelText('Source');
    expect(sourceSelect.querySelector('option[value="PathParam"]')).toBeInTheDocument();
  });
});

describe('RuleForm: detecteur de conflit a la sauvegarde', () => {
  const existingRules = [
    {
      name: 'existing-rule',
      method: 'GET',
      sub_path: null,
      conditions: { all_of: [], any_of: [] },
    },
  ];

  it("sauvegarde directement sans avertissement quand aucun conflit n'est detecte", async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, container, queryByTestId } = render(RuleForm, {
      props: { existingRules, draftPosition: 1, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(queryByTestId('rule-form-conflict-warning')).not.toBeInTheDocument();
  });

  it('affiche un avertissement non bloquant quand un conflit est detecte, sans appeler onSave', async () => {
    checkRuleConflicts.mockResolvedValue({
      conflicts: [{ other_rule_name: 'existing-rule', winner: 'other' }],
    });
    const onSave = vi.fn();
    const { getByLabelText, container, findByTestId, getByRole } = render(RuleForm, {
      props: { existingRules, draftPosition: 1, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);

    const warning = await findByTestId('rule-form-conflict-warning');
    expect(warning).toHaveTextContent('existing-rule');
    expect(getByRole('alert')).toBe(warning);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("permet de sauvegarder quand meme malgre l'avertissement", async () => {
    checkRuleConflicts.mockResolvedValue({
      conflicts: [{ other_rule_name: 'existing-rule', winner: 'draft' }],
    });
    const onSave = vi.fn();
    const { getByLabelText, container, findByTestId, getByTestId } = render(RuleForm, {
      props: { existingRules, draftPosition: 0, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);
    await findByTestId('rule-form-conflict-warning');

    await fireEvent.click(getByTestId('rule-form-conflict-save-anyway-button'));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0][0].name).toBe('new-rule');
  });

  it("permet d'annuler l'avertissement pour continuer a modifier la regle", async () => {
    checkRuleConflicts.mockResolvedValue({
      conflicts: [{ other_rule_name: 'existing-rule', winner: 'other' }],
    });
    const onSave = vi.fn();
    const { getByLabelText, container, findByTestId, getByTestId, queryByTestId } = render(RuleForm, {
      props: { existingRules, draftPosition: 1, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);
    await findByTestId('rule-form-conflict-warning');

    await fireEvent.click(getByTestId('rule-form-conflict-cancel-button'));
    expect(queryByTestId('rule-form-conflict-warning')).not.toBeInTheDocument();
    expect(onSave).not.toHaveBeenCalled();
  });

  it('sauvegarde quand meme si la verification de conflit echoue (fail-open)', async () => {
    checkRuleConflicts.mockRejectedValue(new Error('network unreachable'));
    const onSave = vi.fn();
    const { getByLabelText, container } = render(RuleForm, {
      props: { existingRules, draftPosition: 1, onSave },
    });

    await setInput(getByLabelText('Rule name'), 'new-rule');
    await submitForm(container);

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  });
});

describe('RuleForm: action Proxy masquee pour un service purement mocke', () => {
  it('affiche les deux actions (Mock et Proxy) quand le service a une cible', () => {
    const { getByTestId, queryByTestId } = render(RuleForm, {
      props: { isPurelyMocked: false },
    });

    expect(getByTestId('rule-form-action-mock-radio')).toBeInTheDocument();
    expect(getByTestId('rule-form-action-proxy-radio')).toBeInTheDocument();
    expect(queryByTestId('rule-form-purely-mocked-hint')).not.toBeInTheDocument();
  });

  it("masque l'action Proxy quand le service est purement mocke", () => {
    const { getByTestId, queryByTestId } = render(RuleForm, {
      props: { isPurelyMocked: true },
    });

    expect(getByTestId('rule-form-action-mock-radio')).toBeInTheDocument();
    expect(queryByTestId('rule-form-action-proxy-radio')).not.toBeInTheDocument();
  });

  it("une regle heritee en action=proxy repasse en mock a l'ouverture si le service est purement mocke", async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const staleRule = {
      name: 'stale-proxy-rule',
      action: 'proxy',
      conditions: { all_of: [], any_of: [] },
      response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'ok' }], chaos: null },
    };
    const { container, getByTestId } = render(RuleForm, {
      props: { rule: staleRule, existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);
    // Avertissement obligatoire avant sauvegarde reelle (voir describe
    // dedie ci-dessous) : pas de onSave direct ici.
    expect(onSave).not.toHaveBeenCalled();
    await fireEvent.click(getByTestId('rule-form-stale-proxy-save-anyway-button'));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].action).toBe('mock');
  });
});

describe('RuleForm: avertissement avant de persister le changement proxy -> mock', () => {
  function staleProxyRule(overrides = {}) {
    return {
      name: 'stale-proxy-rule',
      action: 'proxy',
      conditions: { all_of: [], any_of: [] },
      response: { status: 200, headers: [], body: [{ type: 'Literal', value: 'ok' }], chaos: null },
      ...overrides,
    };
  }

  it("affiche un avertissement explicite et bloque la sauvegarde tant qu'il n'est pas confirme", async () => {
    const onSave = vi.fn();
    // Le mock `checkRuleConflicts` est partage entre tous les tests de ce
    // fichier (pas de reset global) : on compare le nombre d'appels avant/
    // apres plutot que de supposer un compteur a zero.
    const conflictCallsBefore = checkRuleConflicts.mock.calls.length;
    const { container, getByTestId } = render(RuleForm, {
      props: { rule: staleProxyRule(), existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);

    const warning = getByTestId('rule-form-stale-proxy-warning');
    expect(warning).toHaveTextContent('Proxy');
    expect(warning).toHaveTextContent('Mock');
    expect(onSave).not.toHaveBeenCalled();
    // Le detecteur de conflit (appel reseau) n'est pas interroge tant que
    // l'avertissement local n'est pas resolu.
    expect(checkRuleConflicts.mock.calls.length).toBe(conflictCallsBefore);
  });

  it('"Enregistrer quand meme" persiste effectivement le changement vers mock', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { container, getByTestId, queryByTestId } = render(RuleForm, {
      props: { rule: staleProxyRule(), existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);
    await fireEvent.click(getByTestId('rule-form-stale-proxy-save-anyway-button'));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].action).toBe('mock');
    expect(queryByTestId('rule-form-stale-proxy-warning')).not.toBeInTheDocument();
  });

  it('"Modifier la regle" referme l\'avertissement sans rien sauvegarder', async () => {
    const onSave = vi.fn();
    const { container, getByTestId, queryByTestId } = render(RuleForm, {
      props: { rule: staleProxyRule(), existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);
    await fireEvent.click(getByTestId('rule-form-stale-proxy-cancel-button'));

    expect(onSave).not.toHaveBeenCalled();
    expect(queryByTestId('rule-form-stale-proxy-warning')).not.toBeInTheDocument();
  });

  it("n'apparait jamais pour une regle mock ordinaire sur un service purement mocke", async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const ordinaryRule = staleProxyRule({ name: 'ordinary-mock-rule', action: 'mock' });
    const { container, queryByTestId } = render(RuleForm, {
      props: { rule: ordinaryRule, existingRules: [], isPurelyMocked: true, onSave },
    });

    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(queryByTestId('rule-form-stale-proxy-warning')).not.toBeInTheDocument();
  });

  it("n'apparait jamais pour une regle proxy sur un service qui a une cible (pas purement mocke)", async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { container, queryByTestId } = render(RuleForm, {
      props: { rule: staleProxyRule(), existingRules: [], isPurelyMocked: false, onSave },
    });

    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].action).toBe('proxy');
    expect(queryByTestId('rule-form-stale-proxy-warning')).not.toBeInTheDocument();
  });
});

// Restauration de la vue d'origine a l'edition (retour 1). Avant cette
// passe, TOUTE regle deja construite via un mode structure atterrissait en
// "Template avance" a la reouverture -- Rule.response_mode (backend) leve
// l'ambiguite.
describe("RuleForm: restauration de la vue d'origine a l'edition (retour 1)", () => {
  it('une regle sauvegardee en JSON "par exemple" reouvre directement la vue assistee (pas le template avance)', () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      response_mode: 'json-paste',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [{ name: 'Content-Type', value: 'application/json' }],
        body: [{ type: 'Template', template: '{"siret":"{{path.siret}}"}' }],
        chaos: null,
      },
    };
    const { container, queryByTestId } = render(RuleForm, { props: { rule } });

    // La vue assistee (paste) est affichee directement, avec le champ deja
    // reconstruit -- pas la zone de collage (parsed=true des le depart), et
    // pas le controle "cle" du mode detail (json-guided).
    const sourceSelect = container.querySelector('[data-testid="json-paste-builder-source-select-0"]');
    expect(sourceSelect).toBeInTheDocument();
    expect(sourceSelect.value).toBe('path');
    expect(queryByTestId('json-paste-builder-textarea')).not.toBeInTheDocument();
    expect(queryByTestId('json-builder-key-input-0')).not.toBeInTheDocument();
  });

  it('une regle sauvegardee en JSON "en detail" reouvre directement la vue detaillee (pas le template avance)', () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      response_mode: 'json-guided',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [{ name: 'Content-Type', value: 'application/json' }],
        body: [{ type: 'Template', template: '{"siret":"{{path.siret}}"}' }],
        chaos: null,
      },
    };
    const { queryByTestId } = render(RuleForm, { props: { rule } });

    const keyInput = queryByTestId('json-builder-key-input-0');
    expect(keyInput).toBeInTheDocument();
    expect(keyInput.value).toBe('siret');
    // Le bouton "Modifier en detail" n'apparait que dans la vue assistee :
    // deja en detail, il n'a pas lieu d'etre.
    expect(queryByTestId('rule-form-open-detail-button')).not.toBeInTheDocument();
  });

  it('une regle sauvegardee en XML "par exemple" reouvre directement la vue assistee, avec le pipe restaure', () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      response_mode: 'xml-paste',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [{ name: 'Content-Type', value: 'application/xml' }],
        body: [{ type: 'Template', template: '<response><siret>{{path.siret | upper}}</siret></response>' }],
        chaos: null,
      },
    };
    const { container, queryByTestId } = render(RuleForm, { props: { rule } });

    const sourceSelect = container.querySelector('[data-testid="xml-paste-builder-source-select-0"]');
    expect(sourceSelect).toBeInTheDocument();
    expect(sourceSelect.value).toBe('path');
    const pipeInput = container.querySelector('[data-testid="xml-paste-builder-pipe-input-0"]');
    expect(pipeInput.value).toBe('upper');
    expect(queryByTestId('xml-paste-builder-textarea')).not.toBeInTheDocument();
  });

  it("une regle sans response_mode (sauvegardee avant l'ajout de ce champ) degrade gracieusement vers l'ancienne heuristique (Template avance)", () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [{ name: 'Content-Type', value: 'application/json' }],
        body: [{ type: 'Template', template: '{"siret":"{{path.siret}}"}' }],
        chaos: null,
      },
    };
    const { queryByTestId } = render(RuleForm, { props: { rule } });

    expect(queryByTestId('rule-form-fragment-template-textarea-0')).toBeInTheDocument();
    expect(queryByTestId('json-paste-builder-source-select-0')).not.toBeInTheDocument();
  });

  it('une regle avec response_mode structure mais un corps qui ne correspond plus a cette forme degrade vers Template avance plutot que de planter', () => {
    const rule = {
      name: 'existing',
      action: 'mock',
      response_mode: 'json-paste',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [],
        body: [{ type: 'Literal', value: 'plain text' }],
        chaos: null,
      },
    };
    const { queryByTestId } = render(RuleForm, { props: { rule } });
    expect(queryByTestId('rule-form-fragment-literal-textarea-0')).toBeInTheDocument();
  });
});

// Fusion Format x Assiste/Detail (retour 3). 5 boutons de Format au lieu de
// 7 boutons de mode a plat ; JSON/XML se declinent en 2 sous-modes via un
// bouton "Modifier en detail" plutot qu'un second niveau de bouton visible
// d'emblee.
describe('RuleForm: fusion Format x Assiste/Detail (retour 3)', () => {
  it('affiche 5 boutons de format (JSON/XML/Texte/Template avance/Vide), pas 7', () => {
    const { container } = render(RuleForm);
    const buttons = container.querySelectorAll('[data-testid^="rule-form-mode-button-"]');
    expect(buttons).toHaveLength(5);
    const testids = [...buttons].map((b) => b.dataset.testid);
    expect(testids).toEqual([
      'rule-form-mode-button-json',
      'rule-form-mode-button-xml',
      'rule-form-mode-button-text',
      'rule-form-mode-button-advanced',
      'rule-form-mode-button-empty',
    ]);
  });

  it('une regle neuve en JSON affiche d\'abord la vue assistee (paste), avec le bouton "Modifier en detail"', async () => {
    const { container, queryByTestId } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));

    expect(queryByTestId('json-paste-builder-textarea')).toBeInTheDocument();
    expect(queryByTestId('rule-form-open-detail-button')).toBeInTheDocument();
  });

  it('"Modifier en detail" revele le mode guide SANS avertissement de perte de donnees (transition sans risque)', async () => {
    const { container, queryByTestId, queryByRole } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));

    expect(queryByTestId('json-builder-add-field-button')).toBeInTheDocument();
    expect(queryByRole('alert')).not.toBeInTheDocument();
  });

  it('re-cliquer sur le meme bouton de format (deja actif) ne reinitialise pas la structure en cours', async () => {
    const { container } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="json-builder-add-field-button"]'));
    const keyInput = container.querySelector('[data-testid="json-builder-key-input-0"]');
    await setInput(keyInput, 'total');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));

    expect(container.querySelector('[data-testid="json-builder-key-input-0"]').value).toBe('total');
  });
});

// Correctif "diagnostic reponse JSON/XML", symptome 2 : convertir un
// template "Template avance" XML valide vers le format XML echouait
// TOUJOURS (tryAdvancedToXmlGuided etait un stub qui ne faisait jamais
// aboutir la conversion, meme pour du XML syntaxiquement correct) alors que
// coller le meme contenu directement dans la vue "par exemple" fonctionnait
// sans probleme (elle ne passe jamais par cette fonction). Desormais alignee
// sur l'equivalent JSON (tryAdvancedToJsonGuided), qui reussissait deja.
describe('RuleForm: conversion Template avance -> XML (correctif symptome 2)', () => {
  async function buildAdvancedXmlTemplate(container, getByLabelText, tpl) {
    await setInput(getByLabelText('Rule name'), 'test-rule');
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-advanced"]'));
    const typeSelect = container.querySelector('[data-testid="rule-form-fragment-type-select-0"]');
    await fireEvent.change(typeSelect, { target: { value: 'Template' } });
    const tplTextarea = container.querySelector('[data-testid="rule-form-fragment-template-textarea-0"]');
    await setInput(tplTextarea, tpl);
  }

  it('un template XML valide en mode avance se convertit vers XML par exemple sans avertissement', async () => {
    const { container, getByLabelText, queryByRole } = render(RuleForm);
    await buildAdvancedXmlTemplate(container, getByLabelText, '<response><nom>ACME</nom></response>');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-xml"]'));

    expect(queryByRole('alert')).not.toBeInTheDocument();
    const sourceSelect = container.querySelector('[data-testid="xml-paste-builder-source-select-0"]');
    expect(sourceSelect).toBeInTheDocument();
    expect(container.querySelector('[data-testid="xml-paste-builder-value-input-0"]').value).toBe('ACME');
  });

  it('les attributs de la racine du template avance sont preserves lors de la conversion', async () => {
    const { container, getByLabelText } = render(RuleForm);
    await buildAdvancedXmlTemplate(container, getByLabelText, '<devisResponse ver="1"><nom>ACME</nom></devisResponse>');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-xml"]'));

    expect(container.querySelector('[data-testid^="xml-paste-builder-attr-source-select-root"]')).toBeInTheDocument();
  });

  it('un template XML invalide en mode avance affiche toujours un avertissement de conversion', async () => {
    const { container, getByLabelText, queryByRole } = render(RuleForm);
    await buildAdvancedXmlTemplate(container, getByLabelText, '<response><nom>ACME</response>');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-xml"]'));

    expect(queryByRole('alert')).toHaveTextContent('Cannot convert:');
  });
});

// Correctif "diagnostic reponse JSON/XML", symptome 3 : la fusion Format x
// Assiste/Detail (retour 3) n'offrait qu'un aller simple ("Modifier en
// detail") vers la vue detail, sans aucun moyen de revenir a la vue "par
// exemple" sans perdre le travail en cours. `backToPasteMode()` est le
// symetrique de `revealDetailMode()` : meme structure de Fields entre les
// deux sous-modes, donc copie directe sans avertissement de perte.
describe('RuleForm: retour vers la vue "par exemple" depuis le detail (correctif symptome 3)', () => {
  it('le bouton retour est absent en vue par exemple et apparait en vue detail', async () => {
    const { container, queryByTestId } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    expect(queryByTestId('rule-form-back-to-paste-button')).not.toBeInTheDocument();

    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));
    expect(queryByTestId('rule-form-back-to-paste-button')).toBeInTheDocument();
  });

  it('JSON : revenir a la vue par exemple depuis le detail preserve le contenu, sans avertissement', async () => {
    const { container, queryByRole } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    const textarea = container.querySelector('[data-testid="json-paste-builder-textarea"]');
    await setInput(textarea, '{"nom":"ACME"}');
    await fireEvent.click(container.querySelector('[data-testid="json-paste-builder-analyze-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));

    await fireEvent.click(container.querySelector('[data-testid="rule-form-back-to-paste-button"]'));

    expect(queryByRole('alert')).not.toBeInTheDocument();
    // La liste de champs deja peuplee s'affiche directement (pas la zone de
    // collage vide) : `startParsed` doit refleter le contenu REEL au moment
    // du remontage du composant, pas un etat fige a l'ouverture du formulaire.
    expect(container.querySelector('[data-testid="json-paste-builder-textarea"]')).not.toBeInTheDocument();
    const sourceSelect = container.querySelector('[data-testid="json-paste-builder-source-select-0"]');
    expect(sourceSelect).toBeInTheDocument();
  });

  it('XML : revenir a la vue par exemple depuis le detail preserve le contenu, sans avertissement', async () => {
    const { container, queryByRole } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-xml"]'));
    const textarea = container.querySelector('[data-testid="xml-paste-builder-textarea"]');
    await setInput(textarea, '<response><nom>ACME</nom></response>');
    await fireEvent.click(container.querySelector('[data-testid="xml-paste-builder-analyze-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));

    await fireEvent.click(container.querySelector('[data-testid="rule-form-back-to-paste-button"]'));

    expect(queryByRole('alert')).not.toBeInTheDocument();
    expect(container.querySelector('[data-testid="xml-paste-builder-textarea"]')).not.toBeInTheDocument();
    expect(container.querySelector('[data-testid="xml-paste-builder-source-select-0"]')).toBeInTheDocument();
  });

  it('un aller-retour detail -> par exemple -> detail conserve les modifications faites en detail', async () => {
    const { container } = render(RuleForm);
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    const textarea = container.querySelector('[data-testid="json-paste-builder-textarea"]');
    await setInput(textarea, '{"nom":"ACME"}');
    await fireEvent.click(container.querySelector('[data-testid="json-paste-builder-analyze-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="json-builder-add-field-button"]'));
    await setInput(container.querySelector('[data-testid="json-builder-key-input-1"]'), 'siret');

    await fireEvent.click(container.querySelector('[data-testid="rule-form-back-to-paste-button"]'));
    await fireEvent.click(container.querySelector('[data-testid="rule-form-open-detail-button"]'));

    expect(container.querySelector('[data-testid="json-builder-key-input-0"]').value).toBe('nom');
    expect(container.querySelector('[data-testid="json-builder-key-input-1"]').value).toBe('siret');
  });
});

// Pipes en mode "par exemple" (retour 2). Round-trip complet : coller un
// exemple, assigner une source + un pipe, verifier que le payload final
// envoye au backend contient bien `{{expr | pipe}}`.
describe('RuleForm: pipes en mode "par exemple" (retour 2)', () => {
  it('un pipe applique en mode JSON par exemple se retrouve dans le template du payload', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const { getByLabelText, container } = render(RuleForm, { props: { onSave } });

    await setInput(getByLabelText('Rule name'), 'pipe-rule');
    await fireEvent.click(container.querySelector('[data-testid="rule-form-mode-button-json"]'));
    const textarea = container.querySelector('[data-testid="json-paste-builder-textarea"]');
    await setInput(textarea, '{"siret":"00000000000000"}');
    await fireEvent.click(container.querySelector('[data-testid="json-paste-builder-analyze-button"]'));

    const sourceSelect = container.querySelector('[data-testid="json-paste-builder-source-select-0"]');
    await fireEvent.change(sourceSelect, { target: { value: 'path' } });
    await setInput(container.querySelector('[data-testid="json-paste-builder-value-input-0"]'), 'siret');
    await setInput(container.querySelector('[data-testid="json-paste-builder-pipe-input-0"]'), 'upper');

    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalled());
    const [payload] = onSave.mock.calls[0];
    expect(payload.response.body[0].template).toContain('{{path.siret | upper}}');
    expect(payload.response_mode).toBe('json-paste');
  });
});

// The response must not depend on which builder is mounted: folding the section, switching the action or the view
// unmounts the builders, and what they showed has to be saved all the same.
describe('RuleForm: the response survives the builders being unmounted', () => {
  const byId = (container, id) => container.querySelector(`[data-testid="${id}"]`);

  async function renderNamed() {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const view = render(RuleForm, { props: { onSave } });
    await setInput(byId(view.container, 'rule-form-name-input'), 'kept');
    return { ...view, onSave };
  }

  async function pasteJson(container, json) {
    await setInput(byId(container, 'json-paste-builder-textarea'), json);
    await fireEvent.click(byId(container, 'json-paste-builder-analyze-button'));
  }

  async function pasteXml(container, xml) {
    await fireEvent.click(byId(container, 'rule-form-mode-button-xml'));
    await setInput(byId(container, 'xml-paste-builder-textarea'), xml);
    await fireEvent.click(byId(container, 'xml-paste-builder-analyze-button'));
  }

  async function savedTemplate(container, onSave) {
    const calls = onSave.mock.calls.length;
    await submitForm(container);
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(calls + 1));
    const [payload] = onSave.mock.calls.at(-1);
    return payload.response.body.map((f) => f.template ?? f.value).join('');
  }

  async function toggleResponseSection(container) {
    await fireEvent.click(byId(container, 'rule-form-response-toggle-button'));
  }

  it('saves the pasted JSON while the response section is folded', async () => {
    const { container, onSave } = await renderNamed();
    await pasteJson(container, '{"id":"42"}');
    await toggleResponseSection(container);

    expect(await savedTemplate(container, onSave)).toBe('{"id":"42"}');
  });

  it('saves the response edited before the rule was switched to proxy', async () => {
    const { container, onSave } = await renderNamed();
    await pasteJson(container, '{"id":"42"}');
    await fireEvent.change(byId(container, 'rule-form-action-proxy-radio'));

    expect(await savedTemplate(container, onSave)).toBe('{"id":"42"}');
  });

  it('keeps the root tag and attributes of pasted XML in the detailed view and back', async () => {
    const { container, onSave } = await renderNamed();
    await pasteXml(container, '<order id="7"><a>x</a></order>');
    await fireEvent.click(byId(container, 'rule-form-open-detail-button'));

    expect(byId(container, 'xml-builder-root-tag-input').value).toBe('order');
    expect(await savedTemplate(container, onSave)).toBe('<order id="7"><a>x</a></order>');

    await fireEvent.click(byId(container, 'rule-form-back-to-paste-button'));
    expect(await savedTemplate(container, onSave)).toBe('<order id="7"><a>x</a></order>');
  });

  it('keeps a root tag renamed in the detailed view when going back to the example', async () => {
    const { container, onSave } = await renderNamed();
    await pasteXml(container, '<order><a>x</a></order>');
    await fireEvent.click(byId(container, 'rule-form-open-detail-button'));
    await setInput(byId(container, 'xml-builder-root-tag-input'), 'reply');
    await fireEvent.click(byId(container, 'rule-form-back-to-paste-button'));

    expect(await savedTemplate(container, onSave)).toBe('<reply><a>x</a></reply>');
  });

  it('keeps the root tag of pasted XML when the section is folded and opened again', async () => {
    const { container, onSave } = await renderNamed();
    await pasteXml(container, '<order id="7"><a>x</a></order>');
    await toggleResponseSection(container);
    await toggleResponseSection(container);

    expect(await savedTemplate(container, onSave)).toBe('<order id="7"><a>x</a></order>');
  });

  it('keeps a JSON array pasted by example when the section is folded and opened again', async () => {
    const { container, onSave } = await renderNamed();
    await pasteJson(container, '[{"id":"1"}]');
    await toggleResponseSection(container);
    await toggleResponseSection(container);

    expect(await savedTemplate(container, onSave)).toBe('[{"id":"1"}]');
  });

  it('keeps a JSON array pasted by example in the detailed view, and says so', async () => {
    const { container, onSave } = await renderNamed();
    await pasteJson(container, '[{"id":"1"}]');
    await fireEvent.click(byId(container, 'rule-form-open-detail-button'));

    expect(byId(container, 'json-builder-array-root-hint')).toBeInTheDocument();
    expect(await savedTemplate(container, onSave)).toBe('[{"id":"1"}]');
  });

  it('reopens a rule whose JSON body by example is an array in that view, and saves it unchanged', async () => {
    checkRuleConflicts.mockResolvedValue({ conflicts: [] });
    const onSave = vi.fn();
    const rule = {
      name: 'list',
      method: 'GET',
      action: 'mock',
      response_mode: 'json-paste',
      conditions: { all_of: [], any_of: [] },
      response: {
        status: 200,
        headers: [],
        body: [{ type: 'Template', template: '[{"id":"{{path.id}}"}]' }],
        chaos: null,
      },
    };
    const { container } = render(RuleForm, { props: { rule, onSave } });

    expect(byId(container, 'json-paste-builder-source-select-0')).toBeInTheDocument();
    expect(await savedTemplate(container, onSave)).toBe('[{"id":"{{path.id}}"}]');
  });
});

describe('RuleForm: the traffic of its own service', () => {
  const captured = {
    remaining_path: '/1',
    path_params: {},
    query_params: {},
    headers: {},
    body: '',
    body_truncated: false,
    content_type: null,
  };
  const log = (group_name, path) => ({
    timestamp: 1,
    service_name: 'users',
    group_name,
    method: 'GET',
    path,
    mode: 'mock',
    captured,
  });

  it('offers the rule tester the requests of its service only, not those of a namesake in another group', async () => {
    getLogs.mockResolvedValue([log('team-a', '/a/users/1'), log('team-b', '/b/users/2'), log(null, '/users/3')]);
    const { container } = render(RuleForm, { props: { serviceName: 'users', groupName: 'team-a' } });

    await waitFor(() => expect(container.querySelector('[data-testid="rule-tester-log-select"]')).not.toBeNull());
    const options = [...container.querySelectorAll('[data-testid="rule-tester-log-select"] option:not([disabled])')];
    expect(options.map((o) => o.textContent)).toEqual([expect.stringContaining('/a/users/1')]);
  });
});

describe('RuleForm: converting a by-example response to the advanced template', () => {
  const byId = (container, id) => container.querySelector(`[data-testid="${id}"]`);

  async function paste(container, format, sample) {
    await fireEvent.click(byId(container, `rule-form-mode-button-${format}`));
    await setInput(byId(container, `${format}-paste-builder-textarea`), sample);
    await fireEvent.click(byId(container, `${format}-paste-builder-analyze-button`));
  }

  it('turns a JSON pasted by example into its template, without a warning', async () => {
    const { container } = render(RuleForm);
    await paste(container, 'json', '{"id":"42"}');
    await fireEvent.click(byId(container, 'rule-form-mode-button-advanced'));

    expect(byId(container, 'rule-form-mode-switch-confirm-button')).toBeNull();
    expect(byId(container, 'rule-form-fragment-template-textarea-0').value).toBe('{"id":"42"}');
  });

  it('turns an XML pasted by example into its template, without a warning', async () => {
    const { container } = render(RuleForm);
    await paste(container, 'xml', '<order id="7"><a>x</a></order>');
    await fireEvent.click(byId(container, 'rule-form-mode-button-advanced'));

    expect(byId(container, 'rule-form-mode-switch-confirm-button')).toBeNull();
    expect(byId(container, 'rule-form-fragment-template-textarea-0').value).toBe('<order id="7"><a>x</a></order>');
  });
});
