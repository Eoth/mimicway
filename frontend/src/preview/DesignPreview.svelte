<!-- The design-system preview: every color role, every scale and the shared classes and components of Phasme, in the
     light and the dark theme side by side. The roles and steps are read from tokens.css itself, so a token added
     there shows here without editing this page. Development only (design-preview.html). -->
<script>
  import tokensSource from '../tokens.css?raw';
  import ServiceCard from '../lib/components/ServiceCard.svelte';
  import RuleList from '../lib/components/RuleList.svelte';
  import StatusBadge from '../lib/components/StatusBadge.svelte';
  import ToggleSwitch from '../lib/components/ToggleSwitch.svelte';
  import Notification from '../lib/components/Notification.svelte';

  const names = (pattern) => [...new Set(tokensSource.match(pattern) ?? [])];
  const roles = names(/--color-[\w-]+(?=:)/g);
  const steps = (family) => names(new RegExp(`--${family}-[\\w-]+(?=:)`, 'g'));
  const themes = ['light', 'dark'];
  const methods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS', 'HEAD'];

  const service = (name, mocked) => ({
    name,
    listen_path: `/${name}/{id}`,
    real_target_url: 'https://api.example.test',
    is_mocked: mocked,
    rewrite_directory_urls: false,
    group_name: null,
    wsdl_mode: 'auto',
    rules: [],
  });
  const rule = (name, method, action, conditions = 0) => ({
    name,
    method,
    action,
    conditions: { all_of: Array(conditions).fill({}), any_of: [] },
  });

  // Resolved values, read from each theme's panel once it is in the page.
  let values = $state({});
  function readValues(panel, theme) {
    const style = getComputedStyle(panel);
    values[theme] = Object.fromEntries(roles.map((name) => [name, style.getPropertyValue(name).trim()]));
  }
</script>

<main class="preview">
  <h1>Phasme</h1>
  <p class="lede">
    The design system of the Mimicway interface: roles and scales from <code>src/tokens.css</code>, shared classes from
    <code>src/app.css</code>, rules in <code>design-system.md</code>.
  </p>

  <div class="themes">
    {#each themes as theme (theme)}
      <section
        class="theme"
        data-theme={theme}
        aria-label="{theme} theme"
        {@attach (panel) => readValues(panel, theme)}
      >
        <h2>{theme}</h2>

        <h3>Color roles</h3>
        <ul class="swatches">
          {#each roles as name (name)}
            <li>
              <span class="swatch" style="background: var({name})"></span>
              <code>{name}</code>
              <span class="value">{values[theme]?.[name] ?? ''}</span>
            </li>
          {/each}
        </ul>

        <h3>Type</h3>
        {#each steps('text') as name (name)}
          <p class="type-sample" style="font-size: var({name})"><code>{name}</code> Mock users-api, 200 OK</p>
        {/each}
        <p class="type-sample"><code>--font-code</code> <code>GET /users/{'{id}'} 12:04:09.318</code></p>

        <h3>Space</h3>
        {#each steps('space') as name (name)}
          <div class="space-row"><code>{name}</code><span class="space-bar" style="width: var({name})"></span></div>
        {/each}

        <h3>Lines and corners</h3>
        <div class="row">
          {#each steps('line') as name (name)}
            <span class="line-sample" style="border-left-width: var({name})"><code>{name}</code></span>
          {/each}
          {#each steps('radius') as name (name)}
            <span class="radius-sample" style="border-radius: var({name})"><code>{name}</code></span>
          {/each}
        </div>

        <h3>Buttons</h3>
        <div class="row">
          <button type="button" class="btn btn-primary">Add a service</button>
          <button type="button" class="btn btn-secondary">Cancel</button>
          <button type="button" class="btn btn-outline">Export</button>
          <button type="button" class="btn btn-danger">Delete</button>
          <button type="button" class="btn btn-danger-outline">Clear</button>
          <button type="button" class="btn btn-primary" disabled>Disabled</button>
        </div>
        <div class="row">
          <button type="button" class="btn btn-sm btn-primary">Small</button>
          <button type="button" class="btn-xs btn-secondary">Extra small</button>
          <button type="button" class="btn-icon" aria-label="Edit">&#9998;</button>
          <button type="button" class="btn-icon btn-icon-s btn-delete" aria-label="Delete">&#10005;</button>
          <button type="button" class="btn-icon btn-icon-xs" aria-label="Move up">&#9650;</button>
          <button type="button" class="btn-close" aria-label="Close">&#10005;</button>
        </div>

        <h3>Fields</h3>
        <div class="form-row">
          <div class="form-field">
            <label for="name-{theme}">Rule name</label>
            <input id="name-{theme}" value="get-user-by-id" />
            <span class="field-hint">Unique identifier of this rule in the service</span>
          </div>
          <div class="form-field">
            <label for="method-{theme}">HTTP method</label>
            <select id="method-{theme}"><option>GET</option></select>
          </div>
        </div>
        <div class="form-field">
          <label for="disabled-{theme}">Disabled</label>
          <input id="disabled-{theme}" value="read only" disabled />
        </div>
        <p class="form-error">The name is already used by another rule.</p>

        <h3>Badges and modes</h3>
        <div class="row">
          <StatusBadge active={true} />
          <StatusBadge active={false} />
          <span class="badge badge-mock">mock</span>
          <span class="badge badge-proxy">proxy</span>
          <span class="badge badge-error">no-rule</span>
          {#each methods as method (method)}<span class="method-badge" data-method={method}>{method}</span>{/each}
        </div>
        <div class="row">
          <span class="badge-pill badge-unknown">Not tested</span>
          <span class="badge-pill badge-testing">Testing</span>
          <span class="badge-pill badge-reachable">Reachable</span>
          <span class="badge-pill badge-unreachable">Unreachable</span>
          <span class="badge-pill badge-expired">Expired</span>
        </div>
        <div class="row">
          <ToggleSwitch label="Mock users-api" name="preview-on-{theme}" checked={true} />
          <ToggleSwitch label="Mock orders-api" name="preview-off-{theme}" checked={false} />
        </div>
        <ServiceCard service={service('users-api', true)} />
        <ServiceCard service={service('orders-api', false)} />
        <RuleList
          rules={[
            rule('get-user', 'GET', 'mock', 1),
            rule('create-user', 'POST', 'mock'),
            rule('real-backend', 'GET', 'proxy'),
          ]}
        />

        <h3>Sections, callouts, notifications</h3>
        <fieldset class="section">
          <legend>Action when this rule matches</legend>
          <p class="section-help">Without any condition, the rule matches every request.</p>
          <div class="sub-section"><strong>Headers</strong>Content-Type: application/json is added automatically.</div>
        </fieldset>
        <div class="callout callout-warning">
          <p class="callout-title">This rule may conflict with an existing rule of this service:</p>
          <ul class="callout-list"><li>The rule "any-order" has identical conditions.</li></ul>
          <div class="callout-actions"><button type="button" class="btn btn-sm btn-primary">Save anyway</button></div>
        </div>
        <div class="callout callout-danger"><p class="callout-title">A script failed to run.</p></div>
        <Notification visible={true} type="success" message="Service saved." />
        <Notification visible={true} type="error" message="The server refused the change." />
        <Notification visible={true} type="info" message="Demo service loaded." />

        <h3>Focus</h3>
        <div class="row">
          <button type="button" class="btn btn-secondary focus-sample">Focused button</button>
          <input class="focus-sample" value="Focused field" aria-label="Focused field" />
        </div>
      </section>
    {/each}
  </div>
</main>

<style>
  .preview {
    padding: var(--space-6);
    max-width: 100rem;
    margin: 0 auto;
  }
  .lede {
    color: var(--color-text-muted);
  }
  .themes {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(32rem, 1fr));
    gap: var(--space-6);
  }
  .theme {
    background: var(--color-bg);
    color: var(--color-text);
    padding: var(--space-5);
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .theme h2 {
    text-transform: capitalize;
    font-size: var(--text-2xl);
  }
  .theme h3 {
    margin: var(--space-4) 0 0;
    font-size: var(--text-l);
  }
  .swatches {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
    gap: var(--space-1) var(--space-3);
  }
  .swatches li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-s);
  }
  .swatch {
    width: var(--size-control);
    height: var(--size-control-xs);
    flex-shrink: 0;
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-s);
  }
  .value {
    margin-left: auto;
    color: var(--color-text-muted);
    font-family: var(--font-code);
  }
  .type-sample {
    margin: 0;
  }
  .type-sample code {
    font-size: var(--text-s);
    color: var(--color-text-muted);
    margin-right: var(--space-2);
  }
  .space-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-s);
  }
  .space-row code {
    width: 7rem;
  }
  .space-bar {
    height: var(--space-2);
    background: var(--color-primary);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .line-sample {
    padding: var(--space-1) var(--space-2);
    border-left: solid var(--color-proxy);
    font-size: var(--text-s);
  }
  .radius-sample {
    padding: var(--space-2);
    border: var(--line-thin) solid var(--color-control);
    font-size: var(--text-s);
  }
  .focus-sample {
    outline: var(--line-thick) solid var(--color-focus);
    outline-offset: var(--line-thick);
  }
</style>
