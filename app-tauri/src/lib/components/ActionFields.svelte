<script lang="ts">
  import type { Action, JsonValue } from '../types';

  export let action: Action;
  export let disabled = false;
  export let onUpdate: (action: Action) => void;

  const modifiers = ['ctrl', 'alt', 'shift', 'super'] as const;
  const mediaCommands = [
    'MUTE',
    'VOL_UP',
    'VOL_DOWN',
    'PLAY_PAUSE',
    'NEXT_TRACK',
    'PREV_TRACK'
  ] as const;

  let jsonError = '';
  $: sinks = outputList(action.sinks);

  function stringValue(field: string): string {
    const value = action[field];
    return typeof value === 'string' ? value : '';
  }

  function updateField(field: string, value: JsonValue): void {
    onUpdate({ ...action, [field]: value });
  }

  function updateModifiers(modifier: string, checked: boolean): void {
    const current = Array.isArray(action.modifiers)
      ? action.modifiers
      : [];
    const updated = checked
      ? current.includes(modifier)
        ? current
        : [...current, modifier]
      : current.filter((value) => value !== modifier);
    const ordered = [
      ...modifiers.filter((value) => updated.includes(value)),
      ...updated.filter(
        (value) => typeof value !== 'string' || !modifiers.some((known) => known === value)
      )
    ];
    updateField('modifiers', ordered);
  }

  function jsonText(value: JsonValue | undefined, fallback: JsonValue): string {
    return JSON.stringify(value === undefined ? fallback : value, null, 2);
  }

  function updateJsonObject(field: string, text: string): void {
    try {
      const value = JSON.parse(text) as JsonValue;
      if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error('Expected an object');
      }
      jsonError = '';
      updateField(field, value);
    } catch {
      jsonError = 'Enter a valid JSON object.';
    }
  }

  function updateDelay(text: string): void {
    const value = Number(text);
    if (!Number.isFinite(value) || value < 0 || value > 600) {
      jsonError = 'Delay must be between 0 and 600 seconds.';
      return;
    }
    jsonError = '';
    updateField('seconds', Math.round(value * 10) / 10);
  }

  function outputList(value: JsonValue | undefined): JsonValue[] {
    return Array.isArray(value) ? value : [];
  }

  function sinkObject(value: JsonValue): value is { [key: string]: JsonValue } {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
  }

  function sinkName(value: JsonValue): string {
    if (typeof value === 'string') return value;
    if (!sinkObject(value)) return '';
    return typeof value.name === 'string' ? value.name : '';
  }

  function updateSinkName(index: number, name: string): void {
    const updated = [...sinks];
    const sink = updated[index];
    updated[index] =
      typeof sink === 'string'
        ? name
        : sinkObject(sink)
          ? { ...sink, name }
          : { name, description: '' };
    updateField('sinks', updated);
  }

  function addSink(): void {
    updateField('sinks', [...sinks, { name: '', description: '' }]);
  }

  function removeSink(index: number): void {
    updateField(
      'sinks',
      sinks.filter((_, sinkIndex) => sinkIndex !== index)
    );
  }

  function moveSink(index: number, delta: number): void {
    const nextIndex = index + delta;
    if (nextIndex < 0 || nextIndex >= sinks.length) return;
    const updated = [...sinks];
    [updated[index], updated[nextIndex]] = [updated[nextIndex], updated[index]];
    updateField('sinks', updated);
  }
</script>

{#if action.type === 'shortcut'}
  <fieldset disabled={disabled}>
    <legend>Modifiers</legend>
    {#each modifiers as modifier}
      <label class="modifier">
        <input
          type="checkbox"
          aria-label={modifier.toUpperCase()}
          checked={Array.isArray(action.modifiers) && action.modifiers.includes(modifier)}
          onchange={(event) => updateModifiers(modifier, event.currentTarget.checked)}
        />
        {modifier.toUpperCase()}
      </label>
    {/each}
  </fieldset>
  <label>
    Key
    <input
      type="text"
      aria-label="Key"
      value={stringValue('key')}
      disabled={disabled}
      onchange={(event) => updateField('key', event.currentTarget.value)}
    />
  </label>
{:else if action.type === 'text'}
  <label>
    Text
    <textarea
      aria-label="Text"
      rows="4"
      value={stringValue('text')}
      disabled={disabled}
      onchange={(event) => updateField('text', event.currentTarget.value)}
    ></textarea>
  </label>
{:else if action.type === 'media'}
  <label>
    Media command
    <select
      aria-label="Media command"
      value={stringValue('mediaKey')}
      disabled={disabled}
      onchange={(event) => updateField('mediaKey', event.currentTarget.value)}
    >
      {#each mediaCommands as command}
        <option value={command}>{command}</option>
      {/each}
    </select>
  </label>
{:else if action.type === 'audio_output'}
  <div class="outputs" aria-label="Audio output list">
    {#if sinks.length === 0}
      <p class="empty-fields">No outputs configured.</p>
    {/if}
    {#each sinks as sink, index (index)}
      <div class="output-row">
        <label>
          Output {index + 1} name
          <input
            aria-label={`Output ${index + 1} name`}
            value={sinkName(sink)}
            disabled={disabled}
            onchange={(event) => updateSinkName(index, event.currentTarget.value)}
          />
        </label>
        <div class="output-controls">
          <button
            type="button"
            aria-label={`Move audio output ${index + 1} up`}
            disabled={disabled || index === 0}
            onclick={() => moveSink(index, -1)}
          >↑</button>
          <button
            type="button"
            aria-label={`Move audio output ${index + 1} down`}
            disabled={disabled || index === sinks.length - 1}
            onclick={() => moveSink(index, 1)}
          >↓</button>
          <button
            type="button"
            aria-label={`Remove audio output ${index + 1}`}
            disabled={disabled}
            onclick={() => removeSink(index)}
          >Remove</button>
        </div>
      </div>
    {/each}
    <button
      class="add-output"
      type="button"
      aria-label="Add audio output"
      {disabled}
      onclick={addSink}
    >+ Add output</button>
  </div>
{:else if action.type === 'launch_app'}
  <label>
    App name
    <input
      aria-label="App name"
      value={stringValue('appName')}
      disabled={disabled}
      onchange={(event) => updateField('appName', event.currentTarget.value)}
    />
  </label>
  <label>
    App path
    <input
      aria-label="App path"
      value={stringValue(action.path !== undefined ? 'path' : 'desktopId')}
      disabled={disabled}
      onchange={(event) =>
        updateField(action.path !== undefined ? 'path' : 'desktopId', event.currentTarget.value)}
    />
  </label>
{:else if action.type === 'system_action'}
  <label>
    System action ID
    <input
      aria-label="System action ID"
      value={stringValue('presetId')}
      disabled={disabled}
      onchange={(event) => updateField('presetId', event.currentTarget.value)}
    />
  </label>
{:else if action.type === 'bash_script'}
  <label>
    Script
    <textarea
      aria-label="Script"
      rows="5"
      value={stringValue('script')}
      disabled={disabled}
      onchange={(event) => updateField('script', event.currentTarget.value)}
    ></textarea>
  </label>
{:else if action.type === 'home_assistant'}
  <label>
    Entity ID
    <input
      aria-label="Entity ID"
      value={stringValue('entityId')}
      disabled={disabled}
      onchange={(event) => updateField('entityId', event.currentTarget.value)}
    />
  </label>
  <label>
    Domain
    <input
      aria-label="Domain"
      value={stringValue('domain')}
      disabled={disabled}
      onchange={(event) => updateField('domain', event.currentTarget.value)}
    />
  </label>
  <label>
    Service
    <input
      aria-label="Service"
      value={stringValue('service')}
      disabled={disabled}
      onchange={(event) => updateField('service', event.currentTarget.value)}
    />
  </label>
  <label>
    Service data JSON object
    <textarea
      aria-label="Service data JSON object"
      rows="4"
      value={jsonText(action.data, {})}
      disabled={disabled}
      onchange={(event) => updateJsonObject('data', event.currentTarget.value)}
    ></textarea>
  </label>
{:else if action.type === 'ha_webhook'}
  <label>
    Webhook URL
    <input
      aria-label="Webhook URL"
      type="url"
      value={stringValue('url')}
      disabled={disabled}
      onchange={(event) => updateField('url', event.currentTarget.value)}
    />
  </label>
{:else if action.type === 'delay'}
  <label>
    Delay seconds
    <input
      aria-label="Delay seconds"
      type="number"
      min="0"
      max="600"
      step="0.1"
      value={typeof action.seconds === 'number' ? action.seconds : 0}
      disabled={disabled}
      onchange={(event) => updateDelay(event.currentTarget.value)}
    />
  </label>
{:else}
  <p class="empty-fields">No editable fields</p>
{/if}

{#if jsonError}
  <p class="field-error" role="alert">{jsonError}</p>
{/if}

<style>
  label {
    display: grid;
    gap: var(--space-1);
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  input,
  select,
  textarea {
    min-width: 0;
    min-height: 2.5rem;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-base);
    color: var(--text);
    font: inherit;
  }

  textarea {
    resize: vertical;
  }

  fieldset {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  legend {
    color: var(--text-muted);
    font-size: 0.8rem;
  }

  .modifier {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .modifier input {
    min-height: auto;
  }

  .outputs {
    display: grid;
    gap: var(--space-2);
  }

  .output-row {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .output-controls {
    display: flex;
    gap: var(--space-1);
  }

  .output-controls button,
  .add-output {
    min-height: 2rem;
    padding: 0 var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    color: var(--text);
    cursor: pointer;
  }

  .output-controls button:disabled,
  .add-output:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .empty-fields,
  .field-error {
    margin: 0;
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .field-error {
    color: var(--error);
  }
</style>
