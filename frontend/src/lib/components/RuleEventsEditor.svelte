<script>
  // The events of a rule that answers with server-sent events (EventStream in src/models/mod.rs): one fieldset per
  // event, its name, its data (a template, like a body) and its delay, then how the stream ends. The server sends the
  // events in this order and checks the bounds when the rule is saved (check_event_stream in
  // src/server/validation.rs); this editor holds no copy of them.
  //
  // Like the builders of RuleResponseSection, it keeps no state of its own: every change goes back through `onUpdate`
  // and `onCutChange`, so that the response section keeps the events when this editor is unmounted (folded fieldset,
  // another format shown).
  import Sentence from './Sentence.svelte';
  import { t } from '../i18n.svelte.js';

  let { events = [], cutAt = null, onUpdate = () => {}, onCutChange = () => {} } = $props();

  function change(idx, patch) {
    onUpdate(events.map((event, i) => (i === idx ? { ...event, ...patch } : event)));
  }

  function addEvent() {
    onUpdate([...events, { event: null, data: '', delay_ms: events.length ? 1000 : 0 }]);
  }

  function removeEvent(idx) {
    onUpdate(events.filter((_, i) => i !== idx));
    // The cut follows the event it replaces; it goes with that event, and moves up with the ones after it.
    if (cutAt === idx) onCutChange(null);
    else if (cutAt !== null && cutAt > idx) onCutChange(cutAt - 1);
  }

  function delayOf(value) {
    const n = Number(value);
    return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
  }
</script>

<div class="events-editor" data-testid="rule-events-editor">
  <p class="field-hint">
    <Sentence
      text={t(
        'Each event is sent after its delay. Its data is a template, like a body, and each of its lines becomes a {0} line.',
      )}
      codes={['data:']}
    />
  </p>

  {#each events as event, idx}
    <fieldset class="event-row" data-testid="rule-events-row-{idx}">
      <legend>{t('Event {0}', idx + 1)}</legend>
      <div class="event-fields">
        <div class="form-field event-name">
          <label for="event-name-{idx}">{t('Event name (optional)')}</label>
          <input
            id="event-name-{idx}"
            type="text"
            value={event.event ?? ''}
            placeholder="message"
            autocomplete="off"
            oninput={(e) => change(idx, { event: e.currentTarget.value.trim() ? e.currentTarget.value : null })}
            data-testid="rule-events-name-input-{idx}"
          />
        </div>
        <div class="form-field event-delay">
          <label for="event-delay-{idx}">{t('Delay before it (ms)')}</label>
          <input
            id="event-delay-{idx}"
            type="number"
            min="0"
            max="60000"
            step="100"
            value={event.delay_ms ?? 0}
            oninput={(e) => change(idx, { delay_ms: delayOf(e.currentTarget.value) })}
            data-testid="rule-events-delay-input-{idx}"
          />
        </div>
        <div class="form-field event-data">
          <label for="event-data-{idx}">{t('Data')}</label>
          <textarea
            id="event-data-{idx}"
            rows="2"
            value={event.data ?? ''}
            oninput={(e) => change(idx, { data: e.currentTarget.value })}
            data-testid="rule-events-data-input-{idx}"></textarea>
        </div>
        <div class="event-remove">
          <button
            type="button"
            class="btn-icon btn-icon-s btn-delete"
            onclick={() => removeEvent(idx)}
            aria-label={t('Delete event {0}', idx + 1)}
            data-testid="rule-events-remove-button-{idx}">&#10005;</button
          >
        </div>
      </div>
    </fieldset>
  {/each}

  <div class="event-add">
    <button type="button" class="btn btn-sm btn-outline" onclick={addEvent} data-testid="rule-events-add-button"
      >{t('+ Event')}</button
    >
  </div>

  <div class="form-field event-end">
    <label for="event-end">{t('End of the stream')}</label>
    <select
      id="event-end"
      value={cutAt === null ? '' : String(cutAt)}
      onchange={(e) => onCutChange(e.currentTarget.value === '' ? null : Number(e.currentTarget.value))}
      aria-describedby="event-end-hint"
      data-testid="rule-events-end-select"
    >
      <option value="">{t('After the last event, the response ends')}</option>
      {#each events as _, idx}
        <option value={String(idx)}>{t('Cut instead of event {0}', idx + 1)}</option>
      {/each}
    </select>
    <span class="field-hint" id="event-end-hint">
      {t(
        'A cut sends the events before it, waits the delay of the event it replaces, then drops the connection: the client sees a transport error, with no end of the response.',
      )}
    </span>
  </div>
</div>

<style>
  .events-editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .event-row {
    border: var(--line-thin) solid var(--color-border);
    border-radius: var(--radius-m);
    padding: var(--space-1) var(--space-3) 0;
    margin: 0;
  }

  .event-row legend {
    font-size: var(--text-s);
    color: var(--color-text-muted);
    padding: 0 var(--space-1);
  }

  .event-fields {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: flex-start;
  }

  .event-name {
    flex: 1;
    min-width: 8rem;
  }

  .event-data {
    flex: 3;
    min-width: 16rem;
  }

  .event-delay {
    width: 9rem;
  }

  .event-row textarea {
    font-family: var(--font-code);
    resize: vertical;
  }

  .event-add {
    display: flex;
  }

  .event-remove {
    align-self: center;
  }

  .event-end {
    max-width: 32rem;
  }
</style>
