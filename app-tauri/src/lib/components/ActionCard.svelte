<script lang="ts">
  import type { Action } from '../types';
  import ActionFields from './ActionFields.svelte';

  export let action: Action;
  export let triggerLabel: string;
  export let index: number;
  export let count: number;
  export let disabled = false;
  export let onUpdate: (index: number, action: Action) => void;
  export let onRemove: (index: number) => void;
  export let onMove: (index: number, delta: number) => void;

  const actionTypes = [
    'none',
    'mode_toggle',
    'shortcut',
    'text',
    'media',
    'audio_output',
    'launch_app',
    'system_action',
    'bash_script',
    'home_assistant',
    'ha_webhook',
    'delay'
  ];

  function updateType(type: string): void {
    onUpdate(index, { ...action, type });
  }
</script>

<article class="action-card" aria-label={`${triggerLabel} action ${index + 1}`}>
  <div class="card-heading">
    <h4>Action {index + 1}</h4>
    <div class="reorder-controls" aria-label={`${triggerLabel} action ${index + 1} ordering`}>
      <button
        type="button"
        aria-label={`Move ${triggerLabel} action ${index + 1} up`}
        disabled={disabled || index === 0}
        onclick={() => onMove(index, -1)}
      >↑</button>
      <button
        type="button"
        aria-label={`Move ${triggerLabel} action ${index + 1} down`}
        disabled={disabled || index === count - 1}
        onclick={() => onMove(index, 1)}
      >↓</button>
      <button
        type="button"
        aria-label={`Remove ${triggerLabel} action ${index + 1}`}
        disabled={disabled}
        onclick={() => onRemove(index)}
      >Remove</button>
    </div>
  </div>

  <label class="type-field">
    Action type
    <select
      aria-label={`${triggerLabel} action ${index + 1} type`}
      value={action.type}
      disabled={disabled}
      onchange={(event) => updateType(event.currentTarget.value)}
    >
      {#if !actionTypes.includes(action.type)}
        <option value={action.type}>Unknown ({action.type})</option>
      {/if}
      {#each actionTypes as type}
        <option value={type}>{type}</option>
      {/each}
    </select>
  </label>

  <ActionFields
    {action}
    {disabled}
    onUpdate={(updatedAction) => onUpdate(index, updatedAction)}
  />
</article>

<style>
  .action-card {
    display: grid;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-base);
  }

  .card-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  h4 {
    margin: 0;
    font-size: 0.9rem;
  }

  .reorder-controls {
    display: flex;
    gap: var(--space-1);
  }

  button {
    min-height: 2rem;
    padding: 0 var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    color: var(--text);
    cursor: pointer;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .type-field {
    display: grid;
    gap: var(--space-1);
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  select {
    min-height: 2.5rem;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-base);
    color: var(--text);
    font: inherit;
  }
</style>
