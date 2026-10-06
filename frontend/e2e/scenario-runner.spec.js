// Replays the scenarios of scenarios/*.scenarios.json (see scenario-runner.js). A scenario only drives the interface:
// each test prepares its data through the API first, and checks after the scenario what the interface does not show
// (what the mock answers, what was saved), with real calls.
import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { runScenario, loadScenario } from './scenario-runner.js';

const API = 'http://localhost:7342/api';
const dataDir = process.env.DATA_PATH || resolve(process.cwd(), '../data');
const CONFIG_FILE = resolve(dataDir, 'mock-config.yaml');
function readConfigFromDisk() {
  return readFileSync(CONFIG_FILE, 'utf-8');
}

function validService(name, overrides = {}) {
  return {
    name,
    listen_path: '/e2e/*',
    real_target_url: 'http://e2e:80',
    is_mocked: true,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
    ...overrides,
  };
}

function validRule(name, overrides = {}) {
  return {
    name,
    method: 'GET',
    sub_path: null,
    action: 'mock',
    pre_script: null,
    script: null,
    post_script: null,
    conditions: { all_of: [], any_of: [] },
    response: {
      status: 200,
      headers: [{ name: 'Content-Type', value: 'application/json' }],
      body: [{ type: 'Literal', value: '{"ok":true}' }],
      chaos: null,
    },
    ...overrides,
  };
}

test.describe('Scenarios: services and rules', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('create a service with the form', async ({ page }) => {
    await runScenario(page, loadScenario('services.scenarios.json', 'Create a service with the form'));
  });

  test('the service form offers the SOAP type', async ({ page }) => {
    await runScenario(page, loadScenario('services.scenarios.json', 'The service form offers the SOAP type'));
  });

  test('create a simple rule', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: validService('scenario-rule-svc') });
    await runScenario(page, loadScenario('rules.scenarios.json', 'Create a simple rule on an existing service'));
  });

  test('show the rules of a service', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('e2e-svc', {
        rules: [
          validRule('rule-alpha'),
          validRule('rule-beta', {
            conditions: {
              all_of: [{ source: { type: 'QueryParam', key: 'id' }, operator: { type: 'Eq', value: '42' } }],
              any_of: [],
            },
          }),
        ],
      }),
    });
    await runScenario(page, loadScenario('rules.scenarios.json', 'Show the rules of a service'));
  });
});

test.describe('Scenarios: home screen, service list and groups', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('load the example service', async ({ page }) => {
    await runScenario(page, loadScenario('home.scenarios.json', 'Load the example service from the empty list'));
  });

  test('the home screen shows the title', async ({ page }) => {
    await runScenario(page, loadScenario('home.scenarios.json', 'The home screen loads with the Mimicway title'));
  });

  test('the list shows a service created through the API', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: validService('ui-test-svc') });
    await runScenario(
      page,
      loadScenario('services.scenarios.json', 'The list shows a service created through the API, in its group'),
    );
  });

  test('the groups page opens from the navigation bar', async ({ page }) => {
    await runScenario(page, loadScenario('groups.scenarios.json', 'Open the groups page from the navigation bar'));
  });

  // The expanded groups are kept in memory outside the service list (group-expansion-state.svelte.js): they survive
  // leaving the list and coming back, not a reload of the page.
  test('an expanded group stays expanded after opening one of its services', async ({ page, request }) => {
    await request.post(`${API}/groups`, { data: { name: 'persist-grp', code: '', admins: [], members: [] } });
    await request.post(`${API}/services`, { data: validService('persist-svc', { group_name: 'persist-grp' }) });
    await runScenario(
      page,
      loadScenario('groups.scenarios.json', 'An expanded group stays expanded after opening one of its services'),
    );
  });

  test('reloading the page collapses an expanded group', async ({ page, request }) => {
    await request.post(`${API}/groups`, { data: { name: 'reload-grp', code: '', admins: [], members: [] } });
    await request.post(`${API}/services`, { data: validService('reload-svc', { group_name: 'reload-grp' }) });
    await runScenario(page, loadScenario('groups.scenarios.json', 'Reloading the page collapses an expanded group'));
  });
});

// A service with two rules, the second one conditioned on a query parameter.
function ruleTestService(name) {
  return validService(name, {
    rules: [
      validRule('rule-alpha'),
      validRule('rule-beta', {
        conditions: {
          all_of: [{ source: { type: 'QueryParam', key: 'id' }, operator: { type: 'Eq', value: '42' } }],
          any_of: [],
        },
      }),
    ],
  });
}

test.describe('Scenarios: rule list, service card and search', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('the add button opens the rule form when the service has rules', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: ruleTestService('e2e-svc') });
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'The add button opens the rule form when the service has rules'),
    );
  });

  test('the edit button opens the rule form', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: ruleTestService('e2e-svc') });
    await runScenario(page, loadScenario('rules.scenarios.json', 'The edit button of a rule opens its form'));
  });

  test('the delete button removes the rule', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: ruleTestService('e2e-svc') });
    await runScenario(page, loadScenario('rules.scenarios.json', 'The delete button removes the rule from the list'));
  });

  test('the switch of a service card turns its mock off', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: ruleTestService('e2e-svc') });
    await runScenario(page, loadScenario('services.scenarios.json', 'Turn the mock of e2e-svc off from its card'));

    // The switch saves the service at once: it now forwards to its target.
    await expect.poll(async () => (await (await request.get(`${API}/services/e2e-svc`)).json()).is_mocked).toBe(false);
  });

  test('a search that matches no service says so', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: ruleTestService('e2e-svc') });
    await request.post(`${API}/services`, { data: validService('other-svc') });
    await runScenario(page, loadScenario('services.scenarios.json', 'A search that matches no service says so'));
  });

  test('cancelling the rule form goes back to the list', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: ruleTestService('e2e-svc') });
    await runScenario(page, loadScenario('rules.scenarios.json', 'Cancelling the rule form goes back to the list'));
  });

  test('the home screen still loads once a service exists', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: validService('security-svc') });
    await runScenario(page, loadScenario('home.scenarios.json', 'The home screen loads with the Mimicway title'));
  });
});

test.describe('Scenarios: groups, service identity and persistence', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('a group is created from its name alone', async ({ page }) => {
    await runScenario(page, loadScenario('groups.scenarios.json', 'Create a group from its name alone'));
  });

  test('a group name may hold accents and spaces', async ({ page }) => {
    await runScenario(
      page,
      loadScenario('groups.scenarios.json', 'Create a group whose name holds accents and spaces'),
    );
  });

  test('groups created in a row never collide on their code', async ({ page }) => {
    await runScenario(
      page,
      loadScenario('groups.scenarios.json', 'Create three groups in a row without a code collision'),
    );
  });

  test('deleting a service keeps the service of the same name in another group', async ({ page, request }) => {
    await request.post(`${API}/groups`, { data: { name: 'ambig-grp-a', code: '', admins: [], members: [] } });
    await request.post(`${API}/groups`, { data: { name: 'ambig-grp-b', code: '', admins: [], members: [] } });
    await request.post(`${API}/services`, { data: validService('ambig-svc', { group_name: 'ambig-grp-a' }) });
    await request.post(`${API}/services`, { data: validService('ambig-svc', { group_name: 'ambig-grp-b' }) });

    await runScenario(
      page,
      loadScenario(
        'services.scenarios.json',
        'Delete a service of a group, and keep the service of the same name in another group',
      ),
    );

    await expect(async () => {
      const stillB = await request.get(`${API}/groups/ambig-grp-b/services/ambig-svc`);
      expect(stillB.status()).toBe(200);
      const goneA = await request.get(`${API}/groups/ambig-grp-a/services/ambig-svc`);
      expect(goneA.status()).toBe(404);
    }).toPass();
  });

  test('deleting a service reports its success, not an error', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: validService('no-crash-svc') });
    await runScenario(
      page,
      loadScenario('services.scenarios.json', 'Deleting a service reports its success, not an error'),
    );
  });

  test('a service with a path parameter shows in the list', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: validService('tpl-test', { listen_path: '/items/{id}' }) });
    await runScenario(
      page,
      loadScenario('services.scenarios.json', 'A service with a path parameter in its listen path shows in the list'),
    );
  });

  test('turning the mock off from the card reaches the configuration file', async ({ page, request }) => {
    await request.post(`${API}/services`, { data: validService('write-behind-svc') });
    await runScenario(
      page,
      loadScenario('services.scenarios.json', 'Turn the mock of write-behind-svc off from its card'),
    );

    // The configuration file is written in the background (write-behind): wait until it holds the change.
    await expect(async () => {
      const yaml = readConfigFromDisk();
      expect(yaml).toContain('name: write-behind-svc');
      expect(yaml).toMatch(/name: write-behind-svc\n(?:.*\n)*?\s*is_mocked: false/);
    }).toPass({ timeout: 5000 });
  });
});

// The rule form asks the server for overlapping rules before saving (POST /api/rule-conflicts). conflict-rule-one is a
// GET with no sub-path and no condition, the most general rule there is: any other GET rule without sub-path or
// condition overlaps it (MatchEngine::find_rule_conflicts).
test.describe('Scenarios: conflict warning between rules', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('conflict-svc', { rules: [validRule('conflict-rule-one')] }),
    });
  });

  test('a rule that overlaps another raises a warning', async ({ page }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'A rule that overlaps an existing one raises a conflict warning'),
    );
  });

  test('a rule that overlaps no other raises none', async ({ page }) => {
    await runScenario(page, loadScenario('rules.scenarios.json', 'A rule that overlaps no other raises no warning'));
  });

  test('save anyway keeps the overlapping rule', async ({ page }) => {
    await runScenario(page, loadScenario('rules.scenarios.json', 'Save a rule anyway despite the conflict warning'));
  });
});

// A purely mocked service has no target (empty real_target_url): it can only mock.
test.describe('Scenarios: purely mocked services', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
  });

  test('create a purely mocked service', async ({ page }) => {
    await runScenario(page, loadScenario('services.scenarios.json', 'Create a purely mocked service with the form'));
  });

  test('turning purely mocked off shows the target again and keeps the rules', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('purely-mocked-existing', { real_target_url: '', rules: [validRule('existing-rule')] }),
    });
    await runScenario(
      page,
      loadScenario(
        'services.scenarios.json',
        'Turning purely mocked off shows the target field again and keeps the rules',
      ),
    );
  });

  test('a rule of a purely mocked service has no proxy action', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('purely-mocked-for-rule', { real_target_url: '' }),
    });
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'A rule of a purely mocked service has no proxy action'),
    );
  });

  test('making a service with a proxy rule purely mocked warns without blocking', async ({ page, request }) => {
    await request.post(`${API}/services`, {
      data: validService('svc-with-proxy-rule', { rules: [validRule('legacy-proxy-rule', { action: 'proxy' })] }),
    });
    await runScenario(
      page,
      loadScenario(
        'services.scenarios.json',
        'Making a service with a proxy rule purely mocked warns, and saves once confirmed',
      ),
    );
  });

  test('saving a leftover proxy rule of a purely mocked service warns, then makes it a mock', async ({
    page,
    request,
  }) => {
    await request.post(`${API}/services`, {
      data: validService('purely-mocked-stale-proxy', {
        real_target_url: '',
        rules: [validRule('stale-proxy-rule', { action: 'proxy' })],
      }),
    });
    await runScenario(
      page,
      loadScenario(
        'rules.scenarios.json',
        'Editing a leftover proxy rule of a purely mocked service warns before saving',
      ),
    );

    // Saving anyway changed the saved rule, from proxy to mock, not only the warning on screen.
    const resp = await request.get(`${API}/services/purely-mocked-stale-proxy`);
    const body = await resp.json();
    const rule = body.rules.find((r) => r.name === 'stale-proxy-rule');
    expect(rule.action).toBe('mock');
    expect(rule.sub_path).toBe('/updated');
  });
});

// Folding is driven here in the JSON builder only. The XML builder has a fold of its own (both keep their folds with
// fold-paths.js), covered by XmlResponseBuilder.test.js.
test.describe('Scenarios: folded JSON objects and advanced options', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('fold-adv-svc', {
        rules: [validRule('fold-adv-existing-rule', { post_script: '"deja configure"' })],
      }),
    });
  });

  test('the advanced options of a new rule start folded', async ({ page }) => {
    await runScenario(page, loadScenario('rules.scenarios.json', 'The advanced options of a new rule start folded'));
  });

  test('the advanced options open by themselves when a post-script exists', async ({ page }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'The advanced options open by themselves for a rule that has a post-script'),
    );
  });

  test('a pre-script survives folding the advanced options', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'Folding the advanced options keeps the pre-script typed in them'),
    );

    // The pre-script typed before folding was saved, not only shown again.
    const resp = await request.get(`${API}/services/fold-adv-svc`);
    const body = await resp.json();
    const rule = body.rules.find((r) => r.name === 'fold-adv-rule');
    expect(rule.pre_script).toBe('"greeting"');
  });

  test('folding a nested JSON object keeps its fields', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'Folding a nested JSON object hides its fields and keeps their content'),
    );

    // The field typed inside the object, then folded away and shown again, is in the saved template.
    const resp = await request.get(`${API}/services/fold-adv-svc`);
    const body = await resp.json();
    const rule = body.rules.find((r) => r.name === 'fold-json-rule');
    expect(rule.response.body[0].template).toContain('"parent"');
    expect(rule.response.body[0].template).toContain('"child"');
    expect(rule.response.body[0].template).toContain('hello');
  });
});

// The two condition forms shown by the guide (matching-rules.md): one rule per value of the SOAPAction header.
test.describe('Scenarios: rules on the SOAPAction header', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, { data: validService('soap-routing-demo') });
  });

  test('one rule per SOAPAction value is set up with a header condition', async ({ page }) => {
    await runScenario(
      page,
      loadScenario(
        'rules.scenarios.json',
        'Set up one rule per SOAPAction header value on a service (doc illustration)',
      ),
    );
  });
});

// The pattern of rhai-scripts.md: the request holds a list, and a script (parse_json, to_json) builds one response
// element per item. The same pattern on XML is covered by the server's tests
// (xml_repetition_pattern_builds_one_response_item_per_request_item, src/server/intercept.rs).
test.describe('Scenarios: one response element per request element', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('repeat-pattern-svc', { listen_path: '/calcul', real_target_url: '' }),
    });
  });

  test('a script answers one element per element of the request', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario(
        'rules.scenarios.json',
        'A script answers one element per element of the request (doc illustration)',
      ),
    );

    // A call with two lines gets two elements, each built from its line.
    const resp = await request.post('http://localhost:7342/repeat-pattern-svc/calcul', {
      data: {
        lines: [
          { sku: 'REF-001', qty: 3 },
          { sku: 'REF-002', qty: 1 },
        ],
      },
    });
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body.count).toBe(2);
    expect(body.lines).toHaveLength(2);
    expect(body.lines[0].sku).toBe('REF-001');
    expect(body.lines[0].qty).toBe(3);
    expect(body.lines[1].sku).toBe('REF-002');
    expect(body.lines[1].qty).toBe(1);
    expect(body.lines[0].lineTotal).toBe(body.lines[0].unitPrice * 3);
  });
});

// A nested XML example is pasted (XmlPasteBuilder.svelte), its child node entered through the breadcrumb, and one of
// its values taken from the path parameter.
test.describe('Scenarios: XML response by example', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('xml-paste-demo', { listen_path: '/quote/{siret}' }),
    });
  });

  test('a response built from an XML example answers the expected XML', async ({ page, request }) => {
    await runScenario(page, loadScenario('rules.scenarios.json', 'Build an XML response from a pasted example'));

    // The pasted SIRET gives way to the one of the call; the name and the namespace attribute of the root, left
    // alone, come out as pasted.
    const resp = await request.get('http://localhost:7342/xml-paste-demo/quote/12345678901234');
    expect(resp.status()).toBe(200);
    const xml = await resp.text();
    expect(xml).toContain('<devisResponse xmlns:x="urn:test">');
    expect(xml).toContain('<nom>ACME Corp</nom>');
    expect(xml).toContain('<siret>12345678901234</siret>');
    expect(xml).not.toContain('00000000000000');
  });
});

// A flat JSON example is pasted (JsonPasteBuilder.svelte) and one of its fields taken from the path parameter.
test.describe('Scenarios: JSON response by example', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('json-paste-demo', { listen_path: '/entreprise/{siret}' }),
    });
  });

  test('a response built from a JSON example answers the expected JSON', async ({ page, request }) => {
    await runScenario(page, loadScenario('rules.scenarios.json', 'Build a JSON response from a pasted example'));

    const resp = await request.get('http://localhost:7342/json-paste-demo/entreprise/44306184100047');
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body.siret).toBe('44306184100047');
    expect(body.nom).toBe('ACME Corp');
    expect(body.actif).toBe(true);
  });
});

// A rule matches a SOAP operation by XPath (Envelope/Body/recherche, namespace prefixes ignored), and its script
// (parse_xml_items) copies the SIRET of the request into the response.
test.describe('Scenarios: XPath condition on a SOAP body', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('soap-extraction-demo', { listen_path: '/service', real_target_url: '' }),
    });
  });

  test('an XPath condition and a script copy the SIRET of a SOAP request into the response', async ({
    page,
    request,
  }) => {
    await runScenario(
      page,
      loadScenario(
        'rules.scenarios.json',
        'Match a SOAP body by XPath and copy one of its values into the response (doc illustration)',
      ),
    );

    // An empty <Header></Header> precedes the Body, as in real SOAP requests: the XPath lookup (walk_xml) must step
    // over it.
    const resp = await request.post('http://localhost:7342/soap-extraction-demo/service', {
      headers: { 'Content-Type': 'text/xml' },
      data: '<SOAP:Envelope><SOAP-ENV:Header></SOAP-ENV:Header><SOAP-ENV:Body><ns3:recherche><ns3:Nom>Test</ns3:Nom><ns3:Siret>98765432109876</ns3:Siret></ns3:recherche></SOAP-ENV:Body></SOAP:Envelope>',
    });
    expect(resp.status()).toBe(200);
    const xml = await resp.text();
    expect(xml).toContain('<siret>98765432109876</siret>');
  });
});

// A condition of a saved rule is reopened by a click and changed (query parameter mode=legacy becomes header
// X-Mode=new-value): what the rule matches must change with it, not only its label.
test.describe('Scenarios: editing a condition in place', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('edit-condition-svc', { listen_path: '/service', real_target_url: '' }),
    });
  });

  test('a condition edited in place changes what the rule matches', async ({ page, request }) => {
    await runScenario(page, loadScenario('rules.scenarios.json', 'Edit a condition of a rule in place'));

    // The new condition matches...
    const withNewHeader = await request.post('http://localhost:7342/edit-condition-svc/service', {
      headers: { 'X-Mode': 'new-value' },
    });
    expect(withNewHeader.status()).toBe(200);

    // ... the old one, replaced rather than added to, no longer does (a purely mocked service answers 404 when no
    // rule matches)...
    const withOldQueryParam = await request.post('http://localhost:7342/edit-condition-svc/service?mode=legacy');
    expect(withOldQueryParam.status()).toBe(404);

    // ... and neither does the old value under the new key, which would mean only the key had changed.
    const withWrongValue = await request.post('http://localhost:7342/edit-condition-svc/service', {
      headers: { 'X-Mode': 'legacy' },
    });
    expect(withWrongValue.status()).toBe(404);
  });
});

// A rule records the view its response was built in (Rule.response_mode, read by computeInitialEditorState in
// RuleResponseSection.svelte), and reopens in it rather than as an advanced template.
test.describe('Scenarios: a response reopens in the view it was built in', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('view-restore-json', { listen_path: '/echo/{siret}' }),
    });
    await request.post(`${API}/services`, {
      data: validService('view-restore-json-detail'),
    });
    await request.post(`${API}/services`, {
      data: validService('view-restore-xml', { listen_path: '/echo/{siret}' }),
    });
  });

  test('a JSON response built by example reopens in that view, its pipe applied', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'A JSON response built by example reopens in that view, with its pipe'),
    );

    // In the reopened view, the scenario turns the pipe from upper to lower before saving again: an answer in lower
    // case shows that this second save is the one kept.
    const resp = await request.get('http://localhost:7342/view-restore-json/echo/abc123');
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body.siret).toBe('abc123');
    expect(body.note).toBe('bonjour');
  });

  test('a JSON response built in detail reopens in the detailed view', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario(
        'rules.scenarios.json',
        'A JSON response built in detail reopens in the detailed view, not as an advanced template',
      ),
    );
  });

  test('an XML response built by example reopens in that view, its pipe applied', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'An XML response built by example reopens in that view, with its pipe'),
    );

    const resp = await request.get('http://localhost:7342/view-restore-xml/echo/abc123');
    expect(resp.status()).toBe(200);
    const xml = await resp.text();
    expect(xml).toContain('<siret>ABC123</siret>');
    expect(xml).toContain('<note>bonjour</note>');
  });
});

// The XPath source of the XML builder reads a value of an XML or SOAP request body, where a JSON pointer finds
// nothing. Here it copies the SIRET of the request through the pipe substr(0,9), which keeps its first nine
// characters; the request has an empty <Header></Header> before its Body, as real SOAP requests do.
test.describe('Scenarios: XPath source of the XML builder', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('xpath-echo-demo', { listen_path: '/service', real_target_url: '' }),
    });
  });

  test('the XPath source copies a value of the SOAP body, cut by a pipe', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario(
        'rules.scenarios.json',
        'Copy a value of the SOAP body into a detailed XML response with the XPath source',
      ),
    );

    const resp = await request.post('http://localhost:7342/xpath-echo-demo/service', {
      headers: { 'Content-Type': 'text/xml' },
      data: '<SOAP:Envelope><SOAP-ENV:Header></SOAP-ENV:Header><SOAP-ENV:Body><ns3:recherche><ns3:Siret>98765432109876</ns3:Siret></ns3:recherche></SOAP-ENV:Body></SOAP:Envelope>',
    });
    expect(resp.status()).toBe(200);
    const xml = await resp.text();
    expect(xml).toContain('<siret>987654321</siret>');
  });
});

// Each scenario moves a response between the views of its builders; the mock's answer shows that nothing was lost
// on the way.
test.describe('Scenarios: response builders keep their data across views', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('json-fold-demo', { listen_path: '/service', real_target_url: '' }),
    });
    await request.post(`${API}/services`, {
      data: validService('advanced-to-xml-demo', { listen_path: '/service', real_target_url: '' }),
    });
    await request.post(`${API}/services`, {
      data: validService('back-to-paste-demo', { listen_path: '/service', real_target_url: '' }),
    });
    await request.post(`${API}/services`, {
      data: validService('script-value-detail-demo', { listen_path: '/service', real_target_url: '' }),
    });
  });

  test('folding an object of a JSON response by example loses no data', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'Folding an object of a JSON response built by example loses no data'),
    );

    // The untouched field and the one filled after folding and unfolding both reach the answer.
    const resp = await request.post('http://localhost:7342/json-fold-demo/service');
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body).toEqual({ client: { nom: 'ACME', siret: '12345678901234' } });
  });

  test('a valid XML advanced template turns into an XML response by example', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'Turn a valid XML advanced template into an XML response by example'),
    );

    // The scenario's last two assertVisible only pass once the template has become fields of the by-example view (a
    // failed conversion shows a warning instead); the answer shows that the content and the root attribute came along.
    const resp = await request.post('http://localhost:7342/advanced-to-xml-demo/service');
    expect(resp.status()).toBe(200);
    const xml = await resp.text();
    expect(xml).toBe('<devisResponse ver="1"><nom>ACME</nom></devisResponse>');
  });

  test('going back from the detailed JSON view to the example keeps every field', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'Going back from the detailed JSON view to the example keeps every field'),
    );

    // The field added in the detailed view and the pasted one both reach the answer.
    const resp = await request.post('http://localhost:7342/back-to-paste-demo/service');
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body).toEqual({ nom: 'ACME', siret: '12345678901234' });
  });

  test('the script result source of the detailed JSON view takes the key to read', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'The script result source of the detailed JSON view asks which key to read'),
    );

    // The key of the script result is typed in a field of its own: without it, the response could not read
    // {{script.nom}}.
    const resp = await request.post('http://localhost:7342/script-value-detail-demo/service');
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(body).toEqual({ nom: 'ACME Corp' });
  });
});

// parse_date, the reverse of date_now, date_past and date_future: the script reads a date of the request in the
// pattern dd/MM/yyyy and answers it in milliseconds since the epoch.
test.describe('Scenarios: parse_date', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    await request.post(`${API}/services`, {
      data: validService('parse-date-demo', { listen_path: '/convert', real_target_url: '' }),
    });
  });

  test('parse_date turns a date of the request into milliseconds', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario(
        'rules.scenarios.json',
        'Turn a date in a custom format into milliseconds with parse_date (doc illustration)',
      ),
    );

    const resp = await request.get('http://localhost:7342/parse-date-demo/convert?date=15/03/2026');
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    // 15/03/2026 00:00:00 UTC, as in parse_date_fr_pattern_date_only (src/engine/script.rs).
    expect(body.ms).toBe(1773532800000);
  });
});

test.describe('Scenarios: rule form states shown in the guide', () => {
  test.beforeEach(async ({ request }) => {
    await request.delete(`${API}/config/reset`);
    for (const name of ['catalog-svc', 'nested-demo', 'chaos-demo', 'fake-demo']) {
      await request.post(`${API}/services`, {
        data: validService(name, { listen_path: '/v1/*', real_target_url: '' }),
      });
    }
  });

  test('AND and OR conditions combine as the guide says', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'Combine AND and OR conditions on a rule (doc illustration)'),
    );

    const call = (channel, version) =>
      request.get(`http://localhost:7342/catalog-svc/v1/items?channel=${channel}`, {
        headers: version ? { 'X-Client-Version': version } : {},
      });
    expect(await (await call('web', '2')).text()).toBe('catalog for v2 clients');
    expect(await (await call('mobile', '2')).text()).toBe('catalog for v2 clients');
    expect((await call('desktop', '2')).status()).toBe(404);
    expect((await call('web', null)).status()).toBe(404);
  });

  test('the breadcrumb enters a nested object without changing what the rule answers', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario(
        'rules.scenarios.json',
        'Enter a nested object of the detailed JSON builder through its breadcrumb (doc illustration)',
      ),
    );

    const resp = await request.get('http://localhost:7342/nested-demo/v1/customer');
    expect(await resp.json()).toEqual({ customer: { address: { city: 'Lyon', postcode: '69000' } } });
  });

  test('the chaos settings are saved with the rule', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'Set the latency and the error rate of the chaos mode (doc illustration)'),
    );

    const services = await (await request.get(`${API}/services`)).json();
    const rule = services.find((s) => s.name === 'chaos-demo').rules.find((r) => r.name === 'slow-and-flaky');
    expect(rule.response.chaos).toMatchObject({
      delay_min_ms: 200,
      delay_max_ms: 800,
      error_rate: 0.2,
      error_status: 503,
    });
  });

  test('fake data replaces the pasted values on every call', async ({ page, request }) => {
    await runScenario(
      page,
      loadScenario('rules.scenarios.json', 'Replace pasted values with fake data (doc illustration)'),
    );

    // The generated values are random, and may even repeat the pasted ones: the saved template says what was chosen.
    const services = await (await request.get(`${API}/services`)).json();
    const rule = services.find((s) => s.name === 'fake-demo').rules.find((r) => r.name === 'random-customer');
    const template = JSON.stringify(rule.response.body);
    expect(template).toContain('{{fake.CompanyName}}');
    expect(template).toContain('{{fake.CityFR}}');
    const body = await (await request.get('http://localhost:7342/fake-demo/v1/customer')).json();
    expect(body).toEqual({ id: '42', company: expect.any(String), city: expect.any(String) });
    expect(body.company).not.toBe('');
  });
});
