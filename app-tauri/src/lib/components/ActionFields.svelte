<script module lang="ts">
  /**
   * The action-type catalogue and the per-type field editors.
   *
   * This module block owns the one description of "what an action type looks
   * like" so the list, the cards and the fields cannot drift apart. It is a
   * mirror of the legacy Linux app's editor (`linux-app/ui/action_editor.py`,
   * `linux-app/config.py`) and of `octopad_core::config::default_action`: the
   * wire keys below are exactly the keys those two write.
   *
   * Two rules hold everywhere in this file:
   *
   * 1. **Editing a field never rewrites another one.** {@link withField} copies
   *    the action and replaces a single key, so metadata this build does not
   *    understand survives every edit. {@link withType} is the one place a
   *    field can be dropped, and it drops exactly the fields that belonged to
   *    the previous type (or that the new type writes fresh) — anything else is
   *    carried over untouched.
   * 2. **An unknown stored value is never silently replaced.** Selects render
   *    the stored value as an extra option instead of falling back to the first
   *    entry, so opening a config written by a newer build cannot rewrite it.
   */
  import type { Action } from '../types';

  export interface FieldOption {
    value: string;
    label: string;
  }

  export type FieldKind =
    | 'text'
    | 'textarea'
    | 'select'
    | 'modifiers'
    | 'sinks'
    | 'json'
    | 'number';

  export interface ActionFieldSpec {
    /** The JSON key, exactly as the core stores it. */
    name: string;
    label: string;
    kind: FieldKind;
    hint?: string;
    options?: FieldOption[];
    min?: number;
    max?: number;
    step?: number;
  }

  export interface ActionTypeSpec {
    type: string;
    label: string;
    hint: string;
    fields: ActionFieldSpec[];
  }

  /** `config.MODIFIERS`, in the order the stored array is normalised to. */
  export const MODIFIERS = ['ctrl', 'alt', 'shift', 'super'] as const;

  const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('');
  const DIGITS = '0123456789'.split('');
  const FUNCTION_KEYS = Array.from({ length: 24 }, (_unused, offset) => `F${offset + 1}`);
  const NAVIGATION = [
    'ENTER',
    'ESC',
    'BACKSPACE',
    'TAB',
    'SPACE',
    'UP',
    'DOWN',
    'LEFT',
    'RIGHT',
    'HOME',
    'END',
    'PAGE_UP',
    'PAGE_DOWN',
    'DELETE',
  ];

  /** Every key `config.KEY_GROUPS` offers, flattened into one list. */
  export const SHORTCUT_KEYS: string[] = [
    ...ALPHABET,
    ...DIGITS,
    ...FUNCTION_KEYS,
    ...NAVIGATION,
  ];

  /** `config.MEDIA_ACTIONS`. */
  export const MEDIA_COMMANDS: FieldOption[] = [
    { value: 'MUTE', label: 'Mute / Unmute' },
    { value: 'VOL_UP', label: 'Volume Up (+)' },
    { value: 'VOL_DOWN', label: 'Volume Down (-)' },
    { value: 'PLAY_PAUSE', label: 'Play / Pause' },
    { value: 'NEXT_TRACK', label: 'Next Track' },
    { value: 'PREV_TRACK', label: 'Previous Track' },
  ];

  /** `system_actions.SYSTEM_ACTION_PRESETS`, by id. */
  export const SYSTEM_PRESETS: FieldOption[] = [
    { value: 'mic_toggle_mute', label: 'Toggle Microphone Mute' },
    { value: 'volume_up_5', label: 'Volume Up (+5%)' },
    { value: 'volume_down_5', label: 'Volume Down (-5%)' },
    { value: 'volume_toggle_mute', label: 'Toggle Speaker Mute' },
    { value: 'lock_screen', label: 'Lock Session' },
    { value: 'open_terminal', label: 'Open New Terminal' },
    { value: 'screenshot_interactive', label: 'Interactive Screenshot' },
    { value: 'media_play_pause', label: 'Media Play / Pause' },
    { value: 'media_next', label: 'Media Next Track' },
    { value: 'media_prev', label: 'Media Previous Track' },
  ];

  /** Delay bounds, matching the legacy `Gtk.SpinButton` range. */
  export const DELAY_MIN = 0;
  export const DELAY_MAX = 600;
  export const DELAY_STEP = 0.1;

  const NO_FIELDS: ActionFieldSpec[] = [];

  const TEXTAREA = (name: string, label: string, hint: string): ActionFieldSpec => ({
    name,
    label,
    kind: 'textarea',
    hint,
  });

  export const ACTION_TYPES: readonly ActionTypeSpec[] = [
    {
      type: 'none',
      label: 'None',
      hint: 'Empty slot. Pick a type from the menu, or remove this action.',
      fields: NO_FIELDS,
    },
    {
      type: 'shortcut',
      label: 'Shortcut',
      hint: 'Sends a keyboard shortcut to the active window.',
      fields: [
        {
          name: 'modifiers',
          label: 'Modifiers',
          kind: 'modifiers',
          hint: 'Held while the key is sent.',
        },
        {
          name: 'key',
          label: 'Key',
          kind: 'select',
          options: SHORTCUT_KEYS.map((key) => ({ value: key, label: key })),
        },
      ],
    },
    {
      type: 'text',
      label: 'Text',
      hint: 'Types this text as keystrokes into the active window.',
      fields: [TEXTAREA('text', 'Text', 'Multi-line text, typed key by key.')],
    },
    {
      type: 'media',
      label: 'Media',
      hint: 'Controls volume or media playback.',
      fields: [{ name: 'mediaKey', label: 'Media command', kind: 'select', options: MEDIA_COMMANDS }],
    },
    {
      type: 'audio_output',
      label: 'Audio Output',
      hint: 'Cycles the default audio output through the chosen devices.',
      fields: [
        {
          name: 'sinks',
          label: 'Outputs',
          kind: 'sinks',
          hint: 'Switched in order; the native output picker arrives in M3.',
        },
      ],
    },
    {
      type: 'launch_app',
      label: 'App',
      hint: 'Launches an installed desktop application.',
      fields: [
        { name: 'appName', label: 'App name', kind: 'text' },
        {
          name: 'desktopId',
          label: 'App path',
          kind: 'text',
          hint: 'Desktop entry id, or a path to the executable.',
        },
      ],
    },
    {
      type: 'system_action',
      label: 'System',
      hint: 'Runs a built-in system action preset.',
      fields: [{ name: 'presetId', label: 'Preset', kind: 'select', options: SYSTEM_PRESETS }],
    },
    {
      type: 'bash_script',
      label: 'Script',
      hint: 'Runs the script with /bin/bash in the background.',
      fields: [
        {
          name: 'script',
          label: 'Script',
          kind: 'textarea',
          hint: 'Bash, run in the background when the trigger fires.',
        },
      ],
    },
    {
      type: 'home_assistant',
      label: 'Home Assistant',
      hint: 'Calls a Home Assistant service on the chosen entity.',
      fields: [
        { name: 'entityId', label: 'Entity', kind: 'text' },
        { name: 'domain', label: 'Domain', kind: 'text' },
        { name: 'service', label: 'Service', kind: 'text' },
        {
          name: 'data',
          label: 'Data',
          kind: 'json',
          hint: 'JSON object sent with the service call.',
        },
      ],
    },
    {
      type: 'ha_webhook',
      label: 'Webhook',
      hint: 'POSTs an empty JSON body to a Home Assistant webhook URL.',
      fields: [{ name: 'url', label: 'URL', kind: 'text' }],
    },
    {
      type: 'delay',
      label: 'Delay',
      hint: 'Pauses the action sequence before the next action.',
      fields: [
        {
          name: 'seconds',
          label: 'Seconds',
          kind: 'number',
          min: DELAY_MIN,
          max: DELAY_MAX,
          step: DELAY_STEP,
        },
      ],
    },
    {
      type: 'mode_toggle',
      label: 'Mode',
      hint: 'Switches between the Desktop and Home Assistant layers.',
      fields: NO_FIELDS,
    },
  ];

  const UNKNOWN_TYPE: ActionTypeSpec = {
    type: 'unknown',
    label: 'Unknown',
    hint: 'This action type is not known to this build. Its data is preserved unchanged.',
    fields: NO_FIELDS,
  };

  const BY_TYPE = new Map(ACTION_TYPES.map((spec) => [spec.type, spec]));

  /** The spec for `type`; an unknown type is read-only, never rewritten. */
  export function actionTypeSpec(type: string): ActionTypeSpec {
    return BY_TYPE.get(type) ?? UNKNOWN_TYPE;
  }

  /** Editable field names of `type` — the only keys an editor may touch. */
  export function actionTypeFields(type: string): ActionFieldSpec[] {
    return actionTypeSpec(type).fields;
  }

  /** Display label, falling back to the raw type id for unknown actions. */
  export function actionTypeLabel(type: string): string {
    return BY_TYPE.get(type)?.label ?? type;
  }

  /**
   * A new action of `type`, with the same defaults `octopad_core::config`
   * writes — the core's `add_action` appends a bare `none`, and switching the
   * type of an existing card starts from this.
   */
  export function defaultAction(type: string): Action {
    switch (type) {
      case 'shortcut':
        return { type, modifiers: ['ctrl', 'alt', 'shift'], key: 'A' };
      case 'text':
        return { type, text: 'Hello World' };
      case 'media':
        return { type, mediaKey: 'VOL_UP' };
      case 'audio_output':
        return { type, sinks: [] };
      case 'launch_app':
        return { type, desktopId: 'code.desktop', appName: 'Visual Studio Code' };
      case 'system_action':
        return { type, presetId: 'mic_toggle_mute', presetName: 'Toggle Microphone Mute' };
      case 'bash_script':
        return { type, script: 'notify-send "OctoPad" "Action!"' };
      case 'home_assistant':
        return { type, domain: '', service: '', entityId: '', friendlyName: '' };
      case 'ha_webhook':
        return { type, url: '' };
      case 'delay':
        return { type, seconds: 0.5 };
      default:
        return { type };
    }
  }

  /** Copy `action` with exactly one field replaced. Nothing else is touched. */
  export function withField(action: Action, field: string, value: unknown): Action {
    return { ...action, [field]: value };
  }

  /**
   * Change an action's type.
   *
   * The previous type's fields are dropped and the new type starts from its
   * defaults; every other key — including anything this build does not
   * understand — is carried over, so a type change is never a silent reset of
   * the whole action.
   */
  export function withType(action: Action, nextType: string): Action {
    if (action.type === nextType) return action;
    const replaced = new Set([
      ...actionTypeFields(action.type).map((field) => field.name),
      ...actionTypeFields(nextType).map((field) => field.name),
    ]);
    const carried: Action = { type: nextType };
    for (const [key, value] of Object.entries(action)) {
      if (key === 'type' || replaced.has(key)) continue;
      carried[key] = value;
    }
    return { ...defaultAction(nextType), ...carried };
  }

  // -- readers -----------------------------------------------------------------
  //
  // A hand-edited config can hold any JSON value in any field; the editors read
  // what they understand and leave the rest alone.

  export function readText(action: Action, field: string): string {
    const value = action[field];
    return typeof value === 'string' ? value : '';
  }

  export function readNumber(action: Action, field: string, fallback: number): number {
    const value = action[field];
    return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
  }

  export function readStrings(action: Action, field: string): string[] {
    const value = action[field];
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
  }

  /** Pretty-printed JSON for object fields, empty text for anything else. */
  export function readJson(action: Action, field: string): string {
    const value = action[field];
    if (value === undefined || value === null) return '';
    if (typeof value === 'object') {
      try {
        return JSON.stringify(value, null, 2);
      } catch {
        return '';
      }
    }
    return String(value);
  }

  /**
   * The stored value first when this build has no option for it.
   *
   * Falling back to the first option would rewrite the config just by opening
   * it, so the unknown value stays selectable and untouched.
   */
  export function optionsWithCurrent(options: FieldOption[], current: string): FieldOption[] {
    if (current === '' || options.some((option) => option.value === current)) return options;
    return [{ value: current, label: current }, ...options];
  }

  /** Modifier list normalised to {@link MODIFIERS} order, as the legacy app does. */
  export function normaliseModifiers(current: string[]): string[] {
    return MODIFIERS.filter((modifier) => current.includes(modifier));
  }
</script>

<script lang="ts">
  /**
   * The field editors for one action, one row per field of its type.
   *
   * The component is a controlled form of the stored action: it renders what
   * the core reported and reports a replacement action when a field changes. It
   * never decides whether the change is accepted — that is the caller's command,
   * and the core's `button-changed` event is what brings the accepted value back.
   */
  interface Props {
    action: Action;
    /** Unique per card, so every generated `id` is unique in the document. */
    idPrefix: string;
    onChange: (action: Action) => void;
  }

  let { action, idPrefix, onChange }: Props = $props();

  const spec = $derived(actionTypeSpec(action.type));
  const fields = $derived(spec.fields);

  /** Per-field validation message; cleared as soon as the field is touched. */
  let fieldError = $state<string | null>(null);

  // A message about one action must not follow the card to the next one, so it
  // is dropped as soon as a different action is rendered.
  $effect(() => {
    action;
    fieldError = null;
  });

  function fieldId(name: string): string {
    return `${idPrefix}-${name}`;
  }

  function commit(next: Action): void {
    fieldError = null;
    onChange(next);
  }

  function commitText(name: string, raw: string): void {
    commit(withField(action, name, raw));
  }

  function commitSelect(name: string, raw: string): void {
    commit(withField(action, name, raw));
  }

  function commitNumber(spec_: ActionFieldSpec, raw: string): void {
    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) {
      fieldError = `${spec_.label} must be a number.`;
      return;
    }
    const min = spec_.min ?? -Infinity;
    const max = spec_.max ?? Infinity;
    const clamped = Math.min(max, Math.max(min, parsed));
    commit(withField(action, spec_.name, clamped));
  }

  function commitJson(name: string, label: string, raw: string): void {
    if (raw.trim() === '') {
      commit(withField(action, name, {}));
      return;
    }
    try {
      commit(withField(action, name, JSON.parse(raw)));
    } catch {
      // Never send a half-typed object: an invalid command would be a failed
      // edit, and a silently dropped field is worse than a visible message.
      fieldError = `${label} must be valid JSON.`;
    }
  }

  function commitModifier(modifier: string, pressed: boolean): void {
    const current = readStrings(action, 'modifiers');
    const next = pressed ? [...current, modifier] : current.filter((item) => item !== modifier);
    commit(withField(action, 'modifiers', normaliseModifiers(next)));
  }

  // -- audio output sinks -----------------------------------------------------

  function sinks(): unknown[] {
    const value = action.sinks;
    return Array.isArray(value) ? value : [];
  }

  function isSinkObject(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
  }

  function commitSinks(next: unknown[]): void {
    commit(withField(action, 'sinks', next));
  }

  function commitSinkField(index: number, key: string, raw: string): void {
    const next = sinks().map((sink, position) => {
      if (position !== index || !isSinkObject(sink)) return sink;
      return { ...sink, [key]: raw };
    });
    commitSinks(next);
  }

  function addSink(): void {
    commitSinks([...sinks(), { name: '', description: '' }]);
  }

  function removeSink(index: number): void {
    commitSinks(sinks().filter((_sink, position) => position !== index));
  }

  function moveSink(index: number, delta: number): void {
    const list = sinks();
    const target = index + delta;
    if (target < 0 || target >= list.length) return;
    const next = list.slice();
    const [moved] = next.splice(index, 1);
    next.splice(target, 0, moved);
    commitSinks(next);
  }
</script>

<div class="fields" data-testid="action-fields" data-action-type={action.type}>
  {#if fields.length === 0}
    <p class="muted fields__hint" data-testid="action-hint">{spec.hint}</p>
  {:else}
    {#each fields as fieldSpec (fieldSpec.name)}
      <div class="fields__row" data-field-row={fieldSpec.name}>
        <label class="fields__label" for={fieldId(fieldSpec.name)}>{fieldSpec.label}</label>

        {#if fieldSpec.kind === 'text'}
          <input
            id={fieldId(fieldSpec.name)}
            class="fields__input"
            type="text"
            data-field={fieldSpec.name}
            value={readText(action, fieldSpec.name)}
            onchange={(event) => commitText(fieldSpec.name, event.currentTarget.value)}
          />

        {:else if fieldSpec.kind === 'textarea'}
          <textarea
            id={fieldId(fieldSpec.name)}
            class="fields__input fields__input--area mono"
            rows="3"
            data-field={fieldSpec.name}
            value={readText(action, fieldSpec.name)}
            onchange={(event) => commitText(fieldSpec.name, event.currentTarget.value)}
          ></textarea>

        {:else if fieldSpec.kind === 'number'}
          <input
            id={fieldId(fieldSpec.name)}
            class="fields__input fields__input--number"
            type="number"
            data-field={fieldSpec.name}
            min={fieldSpec.min}
            max={fieldSpec.max}
            step={fieldSpec.step}
            value={readNumber(action, fieldSpec.name, 0)}
            onchange={(event) => commitNumber(fieldSpec, event.currentTarget.value)}
          />

        {:else if fieldSpec.kind === 'select'}
          <select
            id={fieldId(fieldSpec.name)}
            class="fields__input fields__input--select"
            data-field={fieldSpec.name}
            value={readText(action, fieldSpec.name)}
            onchange={(event) => commitSelect(fieldSpec.name, event.currentTarget.value)}
          >
            {#each optionsWithCurrent(fieldSpec.options ?? [], readText(action, fieldSpec.name)) as option (option.value)}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>

        {:else if fieldSpec.kind === 'modifiers'}
          <div class="fields__modifiers" role="group" aria-label={fieldSpec.label}>
            {#each MODIFIERS as modifier (modifier)}
              <label class="fields__modifier" for={fieldId(`${fieldSpec.name}-${modifier}`)}>
                <input
                  id={fieldId(`${fieldSpec.name}-${modifier}`)}
                  type="checkbox"
                  data-field={fieldSpec.name}
                  data-modifier={modifier}
                  checked={readStrings(action, fieldSpec.name).includes(modifier)}
                  onchange={(event) => commitModifier(modifier, event.currentTarget.checked)}
                />
                {modifier.toUpperCase()}
              </label>
            {/each}
          </div>

        {:else if fieldSpec.kind === 'sinks'}
          <div class="sinks">
            {#each sinks() as sink, index (index)}
              <div class="sinks__row" data-testid="sink-row" data-sink-index={index}>
                {#if isSinkObject(sink)}
                  <input
                    class="fields__input"
                    type="text"
                    aria-label="Output {index + 1}"
                    data-field="name"
                    placeholder="Output identifier"
                    value={typeof sink.name === 'string' ? sink.name : ''}
                    onchange={(event) => commitSinkField(index, 'name', event.currentTarget.value)}
                  />
                  <input
                    class="fields__input"
                    type="text"
                    aria-label="Label {index + 1}"
                    data-field="description"
                    placeholder="Label"
                    value={typeof sink.description === 'string' ? sink.description : ''}
                    onchange={(event) => commitSinkField(index, 'description', event.currentTarget.value)}
                  />
                {:else}
                  <span class="muted sinks__unknown" data-testid="sink-unknown">
                    Unsupported entry {index + 1}
                  </span>
                {/if}
                <button
                  type="button"
                  class="btn sinks__control"
                  data-testid="sink-move-up"
                  disabled={index === 0}
                  onclick={() => moveSink(index, -1)}
                >
                  ↑
                </button>
                <button
                  type="button"
                  class="btn sinks__control"
                  data-testid="sink-move-down"
                  disabled={index === sinks().length - 1}
                  onclick={() => moveSink(index, 1)}
                >
                  ↓
                </button>
                <button
                  type="button"
                  class="btn sinks__control"
                  data-testid="sink-remove"
                  onclick={() => removeSink(index)}
                >
                  ✕
                </button>
              </div>
            {/each}
            <button type="button" class="btn" data-testid="sink-add" onclick={addSink}>
              Add output
            </button>
          </div>

        {:else if fieldSpec.kind === 'json'}
          <textarea
            id={fieldId(fieldSpec.name)}
            class="fields__input fields__input--area mono"
            rows="4"
            data-field={fieldSpec.name}
            value={readJson(action, fieldSpec.name)}
            onchange={(event) => commitJson(fieldSpec.name, fieldSpec.label, event.currentTarget.value)}
          ></textarea>
        {/if}

        {#if fieldSpec.hint}
          <p class="muted fields__hint">{fieldSpec.hint}</p>
        {/if}
      </div>
    {/each}

    {#if fieldError}
      <p class="fields__error" role="alert" data-testid="action-field-error">{fieldError}</p>
    {/if}
  {/if}
</div>

<style>
  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .fields__row {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .fields__label {
    color: var(--text-secondary);
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .fields__input {
    width: 100%;
    min-height: 30px;
    padding: 4px 10px;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background-color: var(--surface-sunken);
    color: var(--text-primary);
    font-family: inherit;
    font-size: var(--font-size-sm);
  }

  .fields__input--area {
    resize: vertical;
    font-family: var(--font-mono);
    font-size: var(--font-size-xs);
  }

  .fields__input--number {
    max-width: 140px;
  }

  .fields__input--select {
    max-width: 320px;
  }

  .fields__hint {
    margin: 0;
    font-size: var(--font-size-xs);
  }

  .fields__error {
    margin: 0;
    color: var(--error);
    font-size: var(--font-size-xs);
  }

  .fields__modifiers {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .fields__modifier {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: 3px 10px;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-pill);
    background-color: var(--surface-inset);
    color: var(--text-secondary);
    font-size: var(--font-size-xs);
    font-weight: 700;
    cursor: pointer;
  }

  .sinks {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .sinks__row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .sinks__unknown {
    flex: 1;
    font-size: var(--font-size-xs);
  }

  .sinks__control {
    flex: none;
    min-height: 26px;
    min-width: 30px;
    padding: 2px 8px;
  }
</style>