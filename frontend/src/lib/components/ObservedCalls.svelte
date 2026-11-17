<script>
  // The calls traffic observation kept for a service (src/server/observation/calls.rs): the list, most recent first,
  // and one call opened in place, its request whole and its response, or why no response came. A call can be sent
  // again by hand (the curl command of lib/curl.js) and its request body downloaded as kept.
  //
  // Nothing loads by itself: the list comes with its button, like the suggestions, and a call's details when it is
  // opened. What is shown is what the server kept: credentials already [redacted], bodies cut at
  // TRAFFIC_OBSERVATION_MAX_BODY_SIZE, which the screen says, with the real size.
  import { getObservedCalls, getObservedCall } from '../api.js';
  import { t, tCount } from '../i18n.svelte.js';
  import { formatDateTimePrecise } from '../format-date.js';
  import { curlCommand, commandRefusal, bodyFileName, bodyNeedsFile } from '../curl.js';

  let { serviceName, groupName = null } = $props();

  let calls = $state([]);
  let loaded = $state(false);
  let loading = $state(false);
  let error = $state('');
  let openId = $state(null);
  // Details by call identifier, kept once read: a call kept never changes.
  let details = $state({});
  let detailErrors = $state({});
  let copied = $state({});

  // A key for element identifiers, unique among the services of the page.
  const domKey = $derived(`${groupName ?? ''}-${serviceName}`.replace(/[^A-Za-z0-9_-]/g, '_'));

  async function refresh() {
    const name = serviceName;
    const group = groupName;
    loading = true;
    error = '';
    try {
      calls = await getObservedCalls(name, group);
      loaded = true;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function toggle(call) {
    const name = serviceName;
    const group = groupName;
    if (openId === call.id) {
      openId = null;
      return;
    }
    openId = call.id;
    if (details[call.id]) return;
    detailErrors[call.id] = '';
    try {
      details[call.id] = await getObservedCall(name, group, call.id);
    } catch (e) {
      detailErrors[call.id] = e.message;
    }
  }

  function address(call) {
    return call.query === null || call.query === undefined ? call.path : `${call.path}?${call.query}`;
  }

  // The outcome in the list, in a few words.
  function outcome(call) {
    switch (call.failure) {
      case null:
      case undefined:
        return String(call.status);
      case 'refused_target':
        return t('Not sent: target refused');
      case 'unreachable':
        return t('No response: backend unreachable');
      case 'timeout':
        return t('No response: no answer in time');
      case 'interrupted':
        return t('Response cut off');
      default:
        return t('No response');
    }
  }

  // Why no response came, in a sentence.
  function reason(cause) {
    switch (cause) {
      case 'refused_target':
        return t(
          'Not sent: the target of this service is outside the targets Mimicway may relay to (PROXY_ALLOWED_TARGETS).',
        );
      case 'unreachable':
        return t(
          'The backend could not be reached: nothing answered at its address (stopped, wrong host name or port).',
        );
      case 'timeout':
        return t('The backend did not answer in time.');
      case 'interrupted':
        return t('The backend began to answer, then the connection broke before the end of its response.');
      default:
        return t('The relay failed for another reason.');
    }
  }

  function hasRedacted(detail) {
    return Object.values(detail.request.headers).includes('[redacted]');
  }

  // Why no command is offered for a call, in words.
  function refusal(reason) {
    switch (reason) {
      default:
        return t(
          'No command is offered for this call: one of its values holds a control character, which a terminal could act on when the command is pasted.',
        );
    }
  }

  async function copyCommand(command, detail) {
    try {
      await navigator.clipboard.writeText(command);
      copied[detail.id] = t('Command copied.');
    } catch {
      copied[detail.id] = t('Copying is not available in this browser: select the command above and copy it.');
    }
  }

  function downloadBody(detail) {
    // A file to save, never a page to open: the media type the caller sent (text/html, say) is not trusted with it.
    const url = URL.createObjectURL(new Blob([detail.request.body.text], { type: 'application/octet-stream' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = bodyFileName(detail);
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
  }
</script>

{#snippet keptBody(body, testid)}
  {#if body.size === 0}
    <p class="call-note">{t('No body.')}</p>
  {:else}
    {#if body.truncated}
      <p class="call-note" data-testid="{testid}-truncated">
        {t(
          'Cut at the size limit of observation: the whole body was {0} bytes. Raise TRAFFIC_OBSERVATION_MAX_BODY_SIZE to keep more.',
          body.size,
        )}
      </p>
    {/if}
    {#if !body.utf8}
      <p class="call-note" data-testid="{testid}-non-utf8">
        {t(
          'Not UTF-8 text: the bytes that are not UTF-8 are replaced here and in what is copied or downloaded, which is not the original byte for byte.',
        )}
      </p>
    {/if}
    <pre class="call-body" data-testid={testid}>{body.text}</pre>
  {/if}
{/snippet}

{#snippet headerList(headers)}
  {#if Object.keys(headers).length === 0}
    <p class="call-note">{t('No header.')}</p>
  {:else}
    <dl class="call-headers">
      {#each Object.entries(headers) as [name, value] (name)}
        <div class="call-header">
          <dt>{name}</dt>
          <dd>{value}</dd>
        </div>
      {/each}
    </dl>
  {/if}
{/snippet}

<div class="observed-calls" data-testid="observed-calls-{serviceName}">
  <h5 id="observed-calls-title-{domKey}">{t('Calls kept')}</h5>
  <p class="panel-hint">
    {t(
      'The calls relayed while this service is observed, the most recent first, those that got no response included. Credentials appear as [redacted].',
    )}
  </p>
  <div class="panel-actions">
    <button
      type="button"
      class="btn btn-sm btn-outline"
      onclick={refresh}
      disabled={loading}
      data-testid="observed-calls-refresh-button-{serviceName}"
    >
      {loading ? t('Loading...') : loaded ? t('Refresh the calls') : t('Show the calls kept')}
    </button>
  </div>

  {#if loaded && calls.length === 0}
    <p class="panel-empty" data-testid="observed-calls-empty-{serviceName}">
      {t('No call kept yet: call this service through the proxy, then refresh.')}
    </p>
  {/if}

  {#if calls.length > 0}
    <p class="call-count">{tCount(calls.length, '{0} call kept.', '{0} calls kept.')}</p>
    <ul class="call-list" aria-labelledby="observed-calls-title-{domKey}">
      {#each calls as call (call.id)}
        {@const open = openId === call.id}
        {@const detail = details[call.id]}
        <li class="call" data-testid="observed-call-{serviceName}-{call.id}">
          <div class="call-row">
            <span class="call-time">{formatDateTimePrecise(call.timestamp)}</span>
            <span class="call-method">{call.method}</span>
            <span class="call-address" title={address(call)}>{address(call)}</span>
            <span class="call-outcome" class:call-failed={!!call.failure}>{outcome(call)}</span>
            <button
              type="button"
              class="btn btn-sm btn-outline call-toggle"
              aria-expanded={open}
              aria-controls="observed-call-detail-{domKey}-{call.id}"
              onclick={() => toggle(call)}
              data-testid="observed-call-toggle-{serviceName}-{call.id}"
            >
              {t('Details')}<span class="sr-only"> {t('of the call {0} {1}', call.method, address(call))}</span>
            </button>
          </div>
          <div
            id="observed-call-detail-{domKey}-{call.id}"
            class="call-detail"
            hidden={!open}
            data-testid="observed-call-detail-{serviceName}-{call.id}"
          >
            {#if open && detailErrors[call.id]}
              <p class="panel-error" role="alert">
                {t(
                  'This call could not be read ({0}): it may no longer be kept. Refresh the list.',
                  detailErrors[call.id],
                )}
              </p>
            {:else if open && !detail}
              <p class="call-note">{t('Loading...')}</p>
            {:else if detail}
              {@const command = curlCommand(detail, window.location.origin)}
              <h6>{t('Request')}</h6>
              <p class="call-line">
                <code data-testid="observed-call-address-{serviceName}-{call.id}"
                  >{detail.method} {address({ path: detail.request.path, query: detail.request.query })}</code
                >
              </p>
              {#if detail.operation}
                <p class="call-note">{t('SOAP operation {0}', detail.operation)}</p>
              {/if}
              {@render headerList(detail.request.headers)}
              {@render keptBody(detail.request.body, `observed-call-request-body-${serviceName}-${call.id}`)}

              {#if detail.failure}
                <h6>{t('No response')}</h6>
                <p class="call-reason" data-testid="observed-call-reason-{serviceName}-{call.id}">
                  {reason(detail.failure.cause)}
                </p>
                <p class="call-note">
                  {t(
                    'Mimicway answered the caller with {0}. Target: {1}',
                    detail.failure.status,
                    detail.failure.target,
                  )}
                </p>
                {#if detail.failure.detail}
                  <p class="call-note">{t('Technical detail: {0}', detail.failure.detail)}</p>
                {/if}
              {:else if detail.response}
                <h6>{t('Response {0}', detail.response.status)}</h6>
                {@render headerList(detail.response.headers)}
                {@render keptBody(detail.response.body, `observed-call-response-body-${serviceName}-${call.id}`)}
              {/if}

              <h6>{t('Send it again')}</h6>
              {#if command}
                <pre class="call-body" data-testid="observed-call-command-{serviceName}-{call.id}">{command}</pre>
              {:else}
                <p class="call-note" data-testid="observed-call-no-command-{serviceName}-{call.id}">
                  {refusal(commandRefusal(detail, window.location.origin))}
                </p>
              {/if}
              {#if command && hasRedacted(detail)}
                <p class="call-note">
                  {t('Credentials are shown as [redacted]: put the real values back before running the command.')}
                </p>
              {/if}
              {#if command && detail.request.body.truncated}
                <p class="call-note">{t('The body was cut: the command sends only the part kept.')}</p>
              {/if}
              {#if command && bodyNeedsFile(detail)}
                <p class="call-note" data-testid="observed-call-body-file-{serviceName}-{call.id}">
                  {t(
                    'The body holds control or invisible characters, which a terminal could act on when the command is pasted: the command reads it from {0}, the file the download gives, to save next to where it runs.',
                    bodyFileName(detail),
                  )}
                </p>
              {/if}
              <div class="call-actions">
                {#if command}
                  <button
                    type="button"
                    class="btn btn-sm btn-primary"
                    onclick={() => copyCommand(command, detail)}
                    data-testid="observed-call-copy-button-{serviceName}-{call.id}"
                  >
                    {t('Copy the command')}
                  </button>
                {/if}
                {#if detail.request.body.size > 0}
                  <button
                    type="button"
                    class="btn btn-sm btn-outline"
                    onclick={() => downloadBody(detail)}
                    data-testid="observed-call-download-button-{serviceName}-{call.id}"
                  >
                    {t('Download the request body')}
                  </button>
                {/if}
              </div>
              <p class="call-note" role="status">{copied[call.id] ?? ''}</p>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  {#if error}
    <p class="panel-error" role="alert" data-testid="observed-calls-error-{serviceName}">{error}</p>
  {/if}
</div>

<style>
  .observed-calls {
    margin-top: var(--space-5);
    min-width: 0;
  }

  h5 {
    margin: 0;
    font-size: var(--text-m);
  }

  h6 {
    margin: var(--space-3) 0 var(--space-1);
    font-size: var(--text-s);
  }

  .panel-hint,
  .call-count {
    margin: var(--space-2) 0 0;
    font-size: var(--text-s);
    color: var(--color-text-muted);
  }

  .panel-actions {
    margin-top: var(--space-3);
  }

  .panel-empty {
    margin: var(--space-3) 0 0;
    font-size: var(--text-m);
    color: var(--color-text-muted);
  }

  .call-list {
    list-style: none;
    margin: var(--space-2) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .call {
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-2) var(--space-3);
    min-width: 0;
  }

  /* One line when it fits; at narrow widths the address keeps the room left, cut, and the button stays in view. */
  .call-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1) var(--space-2);
    font-size: var(--text-s);
    min-width: 0;
  }

  .call-time {
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .call-method {
    font-weight: var(--weight-heavy);
  }

  .call-address {
    flex: 1 1 6rem;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-code);
  }

  .call-outcome {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  .call-failed {
    color: var(--color-danger);
  }

  .call-toggle {
    flex-shrink: 0;
    margin-left: auto;
  }

  .call-detail {
    margin-top: var(--space-2);
    min-width: 0;
  }

  .call-line,
  .call-reason {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .call-note {
    margin: var(--space-1) 0 0;
    font-size: var(--text-s);
    color: var(--color-text-muted);
    overflow-wrap: anywhere;
  }

  .call-note:empty {
    display: none;
  }

  .call-headers {
    margin: var(--space-1) 0 0;
    font-size: var(--text-s);
  }

  .call-header {
    display: flex;
    flex-wrap: wrap;
    gap: 0 var(--space-2);
  }

  .call-header dt {
    font-weight: var(--weight-heavy);
  }

  .call-header dt::after {
    content: ':';
  }

  .call-header dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  /* Whole, never a box that scrolls: one would have to take the keyboard focus to be read. */
  .call-body {
    margin: var(--space-1) 0 0;
    padding: var(--space-2);
    background: var(--color-bg);
    border-radius: var(--radius-s);
    font-size: var(--text-s);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .call-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  code {
    font-size: var(--text-s);
  }

  .panel-error {
    margin: var(--space-3) 0 0;
    font-size: var(--text-s);
    color: var(--color-danger);
  }
</style>
