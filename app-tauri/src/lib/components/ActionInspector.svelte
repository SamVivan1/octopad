<script lang="ts">
  import type { Action, Button, TriggerName } from '../types';
  import ActionListEditor from './ActionListEditor.svelte';

  export let button: Button;
  export let labelDraft: string;
  export let disabled = false;
  export let onLabelChange: (value: string) => void | Promise<void>;
  export let onActionUpdate: (
    trigger: TriggerName,
    index: number,
    action: Action
  ) => void | Promise<void>;
  export let onActionAdd: (trigger: TriggerName) => void | Promise<void>;
  export let onActionRemove: (trigger: TriggerName, index: number) => void | Promise<void>;
  export let onActionMove: (
    trigger: TriggerName,
    index: number,
    delta: number
  ) => void | Promise<void>;

  const triggers: TriggerName[] = ['single', 'double', 'hold'];
</script>

<aside class="inspector" aria-label="Selected key editor">
  <h2>Key {button.index + 1}</h2>
  <label for="key-label">Label</label>
  <input
    id="key-label"
    aria-label="Current key label"
    value={labelDraft}
    disabled={disabled}
    onchange={(event) => onLabelChange(event.currentTarget.value)}
  />

  {#each triggers as trigger}
    <ActionListEditor
      {trigger}
      actionSlot={button[trigger]}
      {disabled}
      onAdd={() => onActionAdd(trigger)}
      onRemove={(index) => onActionRemove(trigger, index)}
      onMove={(index, delta) => onActionMove(trigger, index, delta)}
      onUpdate={(index, action) => onActionUpdate(trigger, index, action)}
    />
  {/each}
</aside>

<style>
  .inspector {
    position: sticky;
    top: var(--space-4);
    display: grid;
    max-height: calc(100vh - 7rem);
    gap: var(--space-3);
    overflow-y: auto;
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-raised);
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  label {
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  input {
    min-width: 0;
    min-height: 2.5rem;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-base);
    color: var(--text);
    font: inherit;
  }

  @media (max-width: 900px) {
    .inspector {
      position: static;
      max-height: none;
    }
  }
</style>
