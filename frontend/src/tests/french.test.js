// The interface in French. With l10n.test.js, this is the only unit test file that loads the French catalogue: the
// others run in English (setup.js), so that rewording a French translation breaks this file alone, where the French
// texts sit together.
//
// It shows the screens of the pseudo-locale test (helpers/screens.js) as a French user sees them after picking the
// language: each screen is mounted in English, switched to French the way the language select does it, then opened.
// A message already on screen when the language changes (a form error, a notification) keeps the language it was
// written in, which is why the states are opened after the switch.
import { render, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest';
import { setLocale } from '../lib/i18n.svelte.js';
import fr from '../locales/fr.json';
import * as api from '../lib/api.js';
import { SCREEN_GROUPS, answerApi, visibleTexts } from './helpers/screens.js';
import RuleTester from '../lib/components/RuleTester.svelte';
import RequestLog from '../lib/components/RequestLog.svelte';

vi.mock('../lib/api.js');

// A few texts each screen shows in French: the catalogue seen in context, on every screen.
const FRENCH_TEXTS = {
  app: ['+ Ajouter un service', 'Sans groupe', 'Configurer'],
  'service-form': ['Nom du service', 'URL cible réelle'],
  'service-detail-proxied': ['Modifier le service', 'Utiliser cette suggestion'],
  'service-detail-mocked': ['+ Ajouter une regle', 'Dupliquer'],
  'rule-form': ['Nom de la regle', 'Tester contre une requête réelle', '✗ Cette règle ne matcherait pas cette requête'],
  'json-builder': ['Champs de la reponse JSON', '+ Ajouter un champ'],
  'json-builder-nested': ['racine', 'Chemin des donnees'],
  'json-builder-array-root': ['La réponse est un tableau : ces champs forment son élément.'],
  'json-paste': ['Recoller un JSON', 'Garder la valeur'],
  'xml-builder': ['Noeuds XML', 'Tag racine :'],
  'xml-paste': ['Recoller un XML', 'Attributs :'],
  'xml-paste-nested': ['racine', 'Chemin des donnees'],
  'json-paste-error': ['Analyser et variabiliser', /^JSON invalide : ./],
  'xml-paste-error': [
    'Analyser et variabiliser',
    'XML invalide : verifiez les tags (noms vides, imbrication incorrecte).',
  ],
  'request-log': ['Journal des requetes', 'Detail de la requete'],
  'messaging-log': ['Simuler un message entrant', 'Detail du message'],
  'group-manager': ['Groupes de services', 'Nom du groupe'],
  'backup-manager': ['Sauvegardes de configuration', 'Restaurer'],
  'tcp-manager': ['Mock TCP brut', 'Ecoute active'],
  'tcp-manager-new': ["Port d'ecoute", 'Creer'],
  'login-form': ['Connexion requise', 'Se connecter'],
};

// The English messages whose French differs and that are no French text themselves: one of them on a French screen
// is a text that did not follow the language. A message with values matches whatever fills its placeholders.
const frenchTexts = new Set(Object.values(fr));
const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const ENGLISH_ONLY = Object.keys(fr)
  .filter((message) => fr[message] !== message && !frenchTexts.has(message))
  .filter((message) => /[A-Za-z]{2,}/.test(message.replace(/\{\d+\}/g, '')))
  .map((message) => ({
    message,
    pattern: new RegExp(
      `^${message
        .split(/\{\d+\}/)
        .map(escape)
        .join('.+')}$`,
      's',
    ),
  }));

const shownTexts = (container) =>
  visibleTexts(container)
    .map(({ text }) => text.replace(/\s+/g, ' ').trim())
    .filter(Boolean);

function englishLeft(texts) {
  return texts.filter((text) => ENGLISH_ONLY.some(({ pattern }) => pattern.test(text)));
}

async function switchToFrench(screen, container) {
  if (screen.id === 'app') {
    // The shell switches through its own select, as a user does; a screen alone through setLocale, which that
    // select calls.
    await waitFor(() => expect(container.querySelector('[data-testid="app-language-select"]')).not.toBeNull());
    await fireEvent.change(container.querySelector('[data-testid="app-language-select"]'), { target: { value: 'fr' } });
    await waitFor(() => expect(document.documentElement.lang).toBe('fr'));
  } else {
    await setLocale('fr');
  }
}

describe('the main screens in French', () => {
  beforeAll(() => answerApi(api));

  afterEach(() => cleanup());

  it('names French texts for every screen, and only for them', () => {
    const ids = SCREEN_GROUPS.flatMap(({ screens }) => screens.map(({ id }) => id));
    expect(Object.keys(FRENCH_TEXTS).sort()).toEqual([...ids].sort());
  });

  for (const { name, screens } of SCREEN_GROUPS) {
    it(name, async () => {
      for (const screen of screens) {
        await setLocale('en');
        const { container } = render(await screen.component(), { props: screen.props ?? {} });
        await switchToFrench(screen, container);
        await screen.open?.(container);
        await new Promise((resolve) => setTimeout(resolve, 0));
        const texts = shownTexts(container);
        expect(englishLeft(texts), screen.id).toEqual([]);
        for (const expected of FRENCH_TEXTS[screen.id]) {
          const found =
            expected instanceof RegExp ? texts.some((text) => expected.test(text)) : texts.includes(expected);
          expect(found, `${screen.id}: ${expected}`).toBe(true);
        }
        cleanup();
      }
    });
  }
});

describe('what French writes its own way', () => {
  afterEach(() => cleanup());

  it('a space before the colon that follows the script of an error', async () => {
    await setLocale('fr');
    api.testRule.mockResolvedValue({
      method_matches: true,
      sub_path_matches: true,
      path_params: {},
      overall_matched: true,
      body_truncated: false,
      all_of: [],
      any_of: [],
      script_errors: [{ slot: 'script', message: 'boom' }],
    });
    const log = {
      timestamp: 1,
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
        query_params: {},
        headers: {},
        body: '',
        body_truncated: false,
        content_type: null,
      },
    };
    const getDraftRule = () => ({ method: 'GET', subPath: '', allOf: [], anyOf: [], script: 'boom()' });
    const { container, getByTestId } = render(RuleTester, {
      props: { serviceName: 'svc-a', logs: [log], getDraftRule },
    });
    await fireEvent.change(container.querySelector('[data-testid="rule-tester-log-select"]'), {
      target: { value: '0' },
    });
    await fireEvent.click(container.querySelector('[data-testid="rule-tester-test-button"]'));
    await waitFor(() => expect(getByTestId('rule-tester-script-error-script')).toBeInTheDocument());
    expect(getByTestId('rule-tester-script-error-script').textContent.trim()).toBe('Script personnalisé : boom');
  });

  it('dates with the day first in the request log', async () => {
    await setLocale('fr');
    const timestamp = new Date('2026-01-15T10:30:00').getTime();
    api.getLogs.mockResolvedValue([
      {
        timestamp,
        service_name: 'svc-a',
        method: 'GET',
        path: '/svc-a/foo',
        mode: 'mock',
        rule_matched: 'r1',
        target_url: null,
        status: 200,
      },
    ]);
    const { container, getByText } = render(RequestLog);
    await waitFor(() => expect(getByText('svc-a')).toBeInTheDocument());
    const shown = container.querySelector('.col-time').textContent;
    expect(shown).toBe(
      new Date(timestamp).toLocaleString('fr-FR', {
        day: '2-digit',
        month: '2-digit',
        year: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
      }),
    );
    expect(shown).toMatch(/^15\/01\/26/);
  });
});
