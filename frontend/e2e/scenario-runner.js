// Replays the scenarios of scenarios/*.scenarios.json: readable lists of UI steps, run in order, with no BDD
// dependency. A domain file holds several scenarios ({ domain, scenarios: [{ scenario, steps }] }). A step names its
// target logically ("component.key"), resolved through selectors.json, the one place that holds the selectors of the
// scenarios, so that a change of markup is fixed there once. Format and how to add a scenario: README.md next to this
// file.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect } from '@playwright/test';
import { docsScreenshot } from './docs-screenshot.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const selectors = JSON.parse(fs.readFileSync(path.join(__dirname, 'selectors.json'), 'utf8'));

// The interface has no router (the view is a state of App.svelte): only its root is a URL. The other views are
// reached by clicking their navigation button; a page joins this map only once the server serves it at its own URL.
const PAGE_PATHS = {
  services: '/',
};

// The CSS selector of a logical name "component.key", its {placeholders} filled from `params` (a card per service
// takes { name: "my-service" }). An unknown name or a missing parameter throws: it is a mistake in the scenario, never
// a failure to hide.
export function resolveTarget(target, params = {}) {
  const [component, key] = String(target).split('.');
  const group = selectors[component];
  if (!group) {
    throw new Error(`selectors.json: unknown component "${component}" (target "${target}")`);
  }
  const template = group[key];
  if (!template) {
    throw new Error(`selectors.json: unknown key "${key}" in component "${component}" (target "${target}")`);
  }
  return template.replace(/\{(\w+)\}/g, (match, name) => {
    if (!(name in params)) {
      throw new Error(`Missing parameter "${name}" for target "${target}" (selector: ${template})`);
    }
    return String(params[name]);
  });
}

async function runStep(page, step) {
  const { action, target, params, value } = step;
  switch (action) {
    case 'goto': {
      const url = PAGE_PATHS[step.page];
      if (url === undefined) {
        throw new Error(
          `goto: unknown page "${step.page}" (only a page the server serves at its own URL joins PAGE_PATHS)`,
        );
      }
      await page.goto(url);
      await page.waitForLoadState('networkidle');
      return;
    }
    case 'click': {
      await page.locator(resolveTarget(target, params)).click();
      return;
    }
    case 'fill': {
      await page.locator(resolveTarget(target, params)).fill(value);
      return;
    }
    case 'selectOption': {
      await page.locator(resolveTarget(target, params)).selectOption(value);
      return;
    }
    case 'assertVisible': {
      await expect(page.locator(resolveTarget(target, params))).toBeVisible();
      return;
    }
    case 'assertHidden': {
      await expect(page.locator(resolveTarget(target, params))).toBeHidden();
      return;
    }
    case 'assertText': {
      await expect(page.locator(resolveTarget(target, params))).toContainText(value);
      return;
    }
    case 'resizeToContent': {
      // What a user does with the resize handle of a text area, until all its text shows without scrolling: for an
      // image that must show a long script whole.
      await page.locator(resolveTarget(target, params)).evaluate((element) => {
        element.style.height = `${element.scrollHeight + element.offsetHeight - element.clientHeight}px`;
      });
      return;
    }
    case 'screenshot': {
      // `target` is the subject of the image, required: one logical name, or several when the image shows several
      // elements together (their params are merged). It is framed in every run; the capture itself only happens when
      // the documentation screenshots are regenerated (docs-screenshot.js). `file` is a plain file name, written to
      // docs/<language>/screenshots/ once per language of the guide.
      if (!target) throw new Error('screenshot: "target" (the subject of the image) is required');
      const subject = [target]
        .flat()
        .map((name) => resolveTarget(name, params))
        .join(', ');
      await docsScreenshot(page, step.file, subject);
      return;
    }
    default:
      throw new Error(
        `Unsupported action "${action}". Actions: goto, click, fill, selectOption, assertVisible, assertHidden, assertText, resizeToContent, screenshot.`,
      );
  }
}

// Runs the steps of a scenario in order. A failing step is thrown again with its number and content, so that a long
// scenario says where it stopped.
export async function runScenario(page, scenario) {
  for (let i = 0; i < scenario.steps.length; i++) {
    const step = scenario.steps[i];
    try {
      await runStep(page, step);
    } catch (e) {
      throw new Error(
        `Scenario "${scenario.scenario}", step ${i + 1}/${scenario.steps.length} (${JSON.stringify(step)}): ${e.message}`,
      );
    }
  }
}

// A whole domain file: { domain, scenarios: [...] }.
export function loadDomain(filename) {
  const p = path.join(__dirname, 'scenarios', filename);
  return JSON.parse(fs.readFileSync(p, 'utf8'));
}

// One scenario of a domain file, by name. An unknown name throws: it is a mistake in the test, never a failure to
// hide.
export function loadScenario(filename, scenarioName) {
  const domain = loadDomain(filename);
  const found = domain.scenarios.find((s) => s.scenario === scenarioName);
  if (!found) {
    throw new Error(`"${filename}" (domain "${domain.domain}") has no scenario named "${scenarioName}"`);
  }
  return found;
}
