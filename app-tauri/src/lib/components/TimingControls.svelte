<script module lang="ts">
  /**
   * The firmware timing contract, in one place.
   *
   * The ranges and the step are not a UI nicety: they are what the pad accepts,
   * and a value outside them would be written to the device verbatim. They are
   * declared here as data so the inputs, the validation and the tests all read
   * the same numbers instead of three hand-copied literals.
   *
   * `octopad-core` deliberately does *not* range-check timings — `AppState`
   * only rejects an unknown field name — so this component is the last place a
   * bad value can be caught before it reaches the device.
   */
  import type { TimingField } from '../types';

  /** Every timing moves in 5 ms steps, as the firmware expects. */
  export const TIMING_STEP = 5;

  export interface TimingFieldSpec {
    /** The `Config` key, the input `id` and the `set_timing_cmd` field name. */
    field: TimingField;
    /** English label shown next to the input. */
    label: string;
    /** One line explaining what the value does on the pad. */
    hint: string;
    min: number;
    max: number;
  }

  export const TIMING_FIELDS: readonly TimingFieldSpec[] = [
    {
      field: 'debounceMs',
      label: 'Debounce',
      hint: 'Contact chatter ignored before a press counts.',
      min: 0,
      max: 1000,
    },
    {
      field: 'clickTimeoutMs',
      label: 'Click timeout',
      hint: 'A second press inside this window is a double click.',
      min: 50,
      max: 2000,
    },
    {
      field: 'holdTimeoutMs',
      label: 'Hold timeout',
      hint: 'Held this long before firing, the hold trigger runs.',
      min: 50,
      max: 3000,
    },
  ];

  export type TimingParse =
    | { ok: true; value: number }
    | { ok: false; message: string };

  /**
   * Parse one timing field's raw text.
   *
   * Every failure carries a message the field shows verbatim, because a
   * silently dropped keystroke is indistinguishable from a timing the pad
   * ignored.
   */
  export function validateTiming(spec: TimingFieldSpec, raw: string): TimingParse {
    const range = `${spec.min}–${spec.max}`;
    const text = raw.trim();
    if (text === '') {
      return { ok: false, message: `${spec.label} must be a number between ${range} ms.` };
    }
    const value = Number(text);
    if (!Number.isFinite(value)) {
      return { ok: false, message: `${spec.label} must be a number.` };
    }
    if (!Number.isInteger(value)) {
      return { ok: false, message: `${spec.label} must be a whole number of milliseconds.` };
    }
    if (value < spec.min || value > spec.max) {
      return { ok: false, message: `${spec.label} must be between ${range} ms.` };
    }
    if (value % TIMING_STEP !== 0) {
      return { ok: false, message: `${spec.label} must be a multiple of ${TIMING_STEP} ms.` };
    }
    return { ok: true, value };
  }
</script>

<script lang="ts">
  /**
   * The three timing inputs.
   *
   * The component owns no config: every input is a controlled form of the value
   * the core last reported, and an accepted edit comes back through the
   * `timing-changed` event. A rejected one — whether this component caught it
   * or the core refused it — puts the stored value back on screen, so the page
   * can never display a timing the pad does not have.
   *
   * Edits travel through the shared `runEdit` queue, so a timing change cannot
   * overtake a selection or action edit the user just triggered.
   */
  import { get } from 'svelte/store';

  import * as api from '../api';
  import { runEdit } from '../editQueue';
  import { appState, lastError, runCommand } from '../state';
  import { timingValue } from '../types';

  const snapshot = $derived($appState);

  /** The stored value as text; a config holding a non-number reads as empty. */
  function storedText(field: TimingField): string {
    const value = timingValue(snapshot?.config[field]);
    return value === null ? '' : String(value);
  }

  type Drafts = Record<TimingField, string>;
  type Messages = Record<TimingField, string | null>;

  const EMPTY_DRAFTS: Drafts = { debounceMs: '', clickTimeoutMs: '', holdTimeoutMs: '' };
  const EMPTY_MESSAGES: Messages = { debounceMs: null, clickTimeoutMs: null, holdTimeoutMs: null };

  let drafts = $state<Drafts>({ ...EMPTY_DRAFTS });
  let messages = $state<Messages>({ ...EMPTY_MESSAGES });

  // Seed the inputs from the snapshot, then follow the core. Written as one
  // effect over the stored values only, so it never fights the user's typing:
  // a draft is replaced when the core reports a *different* stored value.
  let stored: Drafts | null = null;
  $effect(() => {
    const next: Drafts = { ...EMPTY_DRAFTS };
    for (const spec of TIMING_FIELDS) {
      const field = spec.field;
      const text = storedText(field);
      next[field] = text;
      if (stored === null || stored[field] !== text) {
        drafts[field] = text;
        messages[field] = null;
      }
    }
    stored = next;
  });

  /** Reject with a message and put the core's value back on screen. */
  function refuse(field: TimingField, message: string): void {
    messages[field] = message;
    drafts[field] = storedText(field);
  }

  async function commit(spec: TimingFieldSpec, raw: string): Promise<void> {
    const field = spec.field;
    // Keep the control in step with the DOM first: a rejected value has to
    // change the draft for the input to snap back to what the core reports.
    drafts[field] = raw;

    const parsed = validateTiming(spec, raw);
    if (!parsed.ok) {
      refuse(field, parsed.message);
      return;
    }

    messages[field] = null;
    const accepted = await runEdit(() =>
      runCommand(`set ${spec.label.toLowerCase()}`, () => api.setTiming(field, parsed.value))
    );
    if (!accepted) {
      refuse(field, get(lastError) ?? `${spec.label} could not be changed.`);
    }
  }
</script>

<div class="timings" data-testid="timing-controls">
  {#each TIMING_FIELDS as spec (spec.field)}
    <div class="timings__row" data-timing={spec.field}>
      <label class="timings__label" for={spec.field}>{spec.label}</label>
      <input
        id={spec.field}
        class="timings__input"
        type="number"
        inputmode="numeric"
        min={spec.min}
        max={spec.max}
        step={TIMING_STEP}
        data-testid="timing-input"
        data-field={spec.field}
        aria-invalid={messages[spec.field] ? 'true' : undefined}
        aria-describedby={messages[spec.field] ? `${spec.field}-error` : undefined}
        bind:value={drafts[spec.field]}
        onchange={(event) => commit(spec, event.currentTarget.value)}
      />
      <span class="timings__unit" aria-hidden="true">ms</span>
      <p class="timings__hint muted">{spec.hint}</p>
    </div>

    {#if messages[spec.field]}
      <p
        class="timings__error"
        id="{spec.field}-error"
        role="alert"
        data-testid="timing-error"
        data-field={spec.field}
      >
        {messages[spec.field]}
      </p>
    {/if}
  {/each}
</div>

<style>
  .timings {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .timings__row {
    display: grid;
    grid-template-columns: max-content 96px max-content;
    align-items: center;
    gap: var(--space-2) var(--space-3);
  }

  .timings__label {
    font-size: var(--font-size-md);
    font-weight: 600;
  }

  .timings__input {
    min-height: 30px;
    padding: 4px 8px;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background-color: var(--surface-base);
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: var(--font-size-sm);
  }

  .timings__input[aria-invalid='true'] {
    border-color: var(--error);
  }

  .timings__unit {
    color: var(--text-muted);
    font-size: var(--font-size-sm);
  }

  /* The hint sits on its own line so the control row stays compact. */
  .timings__hint {
    grid-column: 1 / -1;
    margin: 0;
    font-size: var(--font-size-xs);
  }

  .timings__error {
    margin: 0;
    padding-left: var(--space-1);
    border-left: 3px solid var(--error);
    color: var(--error);
    font-size: var(--font-size-sm);
  }

  @media (max-width: 640px) {
    .timings__row {
      grid-template-columns: 1fr auto;
    }

    .timings__label {
      grid-column: 1 / -1;
    }
  }
</style>