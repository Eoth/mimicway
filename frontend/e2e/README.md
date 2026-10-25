# End-to-end tests

Playwright tests that drive a real Mimicway binary through a real browser. Two styles live side by side:

- **Scenarios** (`scenarios/*.scenarios.json`), replayed by `scenario-runner.js`: readable lists of UI steps that target elements by logical names, never by hard-coded selectors.
- **Classic specs** (`*.spec.js`, `*.spec.mjs`): for what scenarios do not express (API-only checks, dedicated server instances, complex assertions).

## Run

```bash
# From the repository root: build the UI, then the binary (which embeds it), and start it with an empty data directory
(cd frontend && npm run build) && cargo build
mkdir -p data
DATA_PATH=./data ./target/debug/mimicway &

cd frontend
npx playwright install chromium   # once
npm run test:e2e
```

The suite reads the interface in English, its source language (`playwright.config.js`); `i18n.spec.js` checks the French interface and the language switch. A few specs start their own Mimicway (authentication, API base URL); they need `cargo build` first. The Kafka specs are skipped unless the binary was built with `--features messaging-kafka`.

## Layout

```
e2e/
  selectors.json            the only place that maps logical names to [data-testid=...] selectors
  scenario-runner.js        minimal step interpreter (no Gherkin, no BDD dependency)
  scenario-runner.spec.js   loads and replays the scenarios below
  scenarios/
    home.scenarios.json       home screen, demo service
    groups.scenarios.json     group creation, collapsible groups
    rules.scenarios.json      rule creation, edition, deletion on a service
    services.scenarios.json   service creation, search, mock switch, same names in different groups
  *.spec.js / *.spec.mjs    classic specs
  docs-screenshot.js        frames the subject of a guide image; writes docs/<language>/screenshots/*.png when DOCS_SCREENSHOTS is set
  docs-screenshot.spec.js   checks that framing guard
  demo-tour.spec.js         the demo at the top of the README, screen by screen
```

## Scenario files

One file per functional domain, holding several scenarios:

```json
{
  "domain": "services",
  "scenarios": [
    {
      "scenario": "Create a service with the form",
      "steps": [
        { "action": "goto", "page": "services" },
        { "action": "click", "target": "app.addServiceButton" },
        { "action": "fill", "target": "serviceForm.nameInput", "value": "my-service" },
        { "action": "click", "target": "serviceForm.submitButton" },
        { "action": "assertVisible", "target": "serviceDetail.editButton" }
      ]
    }
  ]
}
```

- `target` is `"component.key"`, as written in `selectors.json`.
- A repeated element (a card per service, for instance) has placeholders in its selector, filled by `params`: `{ "action": "click", "target": "serviceCard.configureButton", "params": { "name": "my-service" } }`.
- `value` is required by `fill`, `selectOption` and `assertText`.
- Actions: `goto`, `click`, `fill`, `selectOption`, `assertVisible`, `assertHidden`, `assertText`, `resizeToContent`, `screenshot`. Add one only for a real need: the runner stays minimal.
- `resizeToContent` makes a text area as tall as its text, as a user would with its resize handle: for an image that must show a long script whole.
- `goto` takes a `page`, not a URL: the UI has no router, so only `"services"` (the root) is mapped; other views are reached by clicking their navigation button (`app.navLogsButton`, `app.navGroupsButton`…).
- Scenarios only interact with the UI. Data setup (creating a service through the API, resetting the configuration) belongs to the spec that replays the scenario (`test.beforeEach`, `request.post(...)`), as in `scenario-runner.spec.js`.

Add a scenario to the domain file it belongs to, with a name unique in that file; a new domain is a deliberate choice, listed above. Scenario names, like test titles and comments, are written in English: CI fails otherwise (`scripts/check-french-comments.mjs`). Replay it from a spec:

```js
import { test } from '@playwright/test';
import { runScenario, loadScenario } from './scenario-runner.js';

test('create a service', async ({ page }) => {
  await runScenario(page, loadScenario('services.scenarios.json', 'Create a service with the form'));
});
```

`loadScenario(file, name)` fails clearly when the file or the name does not exist; `loadDomain(file)` returns a whole domain. When a step fails, the error names the scenario, the step number and the step itself.

## Selectors

`selectors.json` groups logical names by component (`serviceForm`, `ruleList`, `groupManager`…), each mapped to `[data-testid="..."]`. To add one:

1. Make sure the component has the `data-testid` (convention `{component-kebab}-{element-role}[-{discriminant}]`); add it to the Svelte component first if needed. Never register a selector that matches nothing.
2. Add the entry under its component with a camelCase key (`nameInput`, `submitButton`).
3. For a repeated element, use a placeholder named like the `params` key that fills it: `"card": "[data-testid=\"service-card-{name}\"]"`.

A scenario never writes a selector itself (classic specs locate their elements in their own code). To compare `selectors.json` with the code:

```bash
grep -rhoE 'data-testid="[^"]*"' frontend/src --include="*.svelte" | sort -u
```

## Documentation screenshots

The images of `docs/` are taken by this suite, so they follow the interface instead of going stale. The guide exists in English and French with the same pages, so every image exists in both languages.

- Every image names its **subject**, the element (or elements) it is taken to show. In a scenario, a `{ "action": "screenshot", "file": "name.png", "target": "ruleForm.conflictWarning" }` step captures the page at that point; `target` is required and may list several logical names that must show together (`"target": ["ruleForm.fragmentTemplateTextarea", "rhaiScriptEditor.textarea"]`, their `params` merged). In a spec, `await docsScreenshot(page, 'name.png', '<CSS selector of the subject>')` does the same.
- The subject is scrolled into view when it is not entirely there, only as far as needed, so a screen already framed keeps its position and its open lists. The call then fails when the subject matches nothing, is not displayed, hides part of its own content (a text area scrolled inside: see `resizeToContent`), or does not fit entirely in the 1280×720 view, in either language. This check runs in the standard run too, so CI fails when a change of the interface pushes the subject of an image out of it; `docs-screenshot.spec.js` shows each case.
- Capturing only happens when `DOCS_SCREENSHOTS` is set: the standard run takes no screenshot.
- Each call captures the page in English, switches it to French with the interface's language selector (no reload: the screen keeps its state), captures it again and switches back, so the test goes on in English. A screen without the navigation bar (the login screen) has no selector: pass `{ reopenWaitingFor: '<selector>' }` and the French image is taken by opening the same URL in a French browser. Without that option, a missing selector fails the test rather than producing an image in the wrong language.
- `playwright.docs-screenshots.config.js` sets `DOCS_SCREENSHOTS` and runs only the files that take screenshots, found by reading them (a call to `docsScreenshot(` or `runScenario(`): there is no list to keep up to date.

```bash
npm run docs:screenshots   # from frontend/, with Mimicway running on :7342
```

Images are written to `docs/en/screenshots/` and `docs/fr/screenshots/` under the names the pages reference. CI fails when a page references an image that does not exist (`node scripts/check-doc-links.mjs`), and when an image, a page, a heading or a language link exists in one language only (`node scripts/check-doc-translations.mjs`). The three Kafka images need a binary built with `--features messaging-kafka`. A state of the interface gets its image from the test that reaches it, next to the assertions on what the image shows: a page that describes a state no test reaches has no image to show.

## The demo of the README

The animated demo at the top of the README is made of real screens: `demo-tour.spec.js` goes through a first mock (services, the URL to call, a condition, the rule tester, fake data, the proxy choice, the request log) and checks each screen like a guide image, in every run. With `docsScreenshot(page, name, subject, { directory })`, its frames go to `test-results/demo-tour/<language>/` instead of the guide, and their captions to `captions.json`.

```bash
npm run docs:demo   # from frontend/, with Mimicway running on :7342 and Python's Pillow installed
```

`scripts/make-demo-tour.py` scales each frame to 960 px, prints its caption below it and writes `docs/<language>/screenshots/demo-tour.webp`, one screen every three seconds, and refuses a file over 150,000 bytes. CI refuses any image over that weight, any video or GIF, and an SVG that could run something or keeps moving for a reader who asked for less motion (`node scripts/check-images.mjs`). The frames show the address of a default installation, `http://localhost:7342`, whatever port the run uses.
