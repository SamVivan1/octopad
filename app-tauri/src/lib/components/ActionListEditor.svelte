<script lang="ts">
  import type { Action, ActionSlot, JsonValue, TriggerName } from '../types';
  import ActionCard from './ActionCard.svelte';

  export let trigger: TriggerName;
  export let actionSlot: ActionSlot;
  export let disabled = false;
  export let onAdd: () => void;
  export let onRemove: (index: number) => void;
  export let onMove: (index: number, delta: number) => void;
  export let onUpdate: (index: number, action: Action) => void;

  $: triggerLabel = trigger[0].toUpperCase() + trigger.slice(1);
  $: actions = actionsFrom(actionSlot);

  function isAction(value: JsonValue): value is Action {
    return (
      value !== null &&
      typeof value === 'object' &&
      !Array.isArray(value) &&
      typeof value.type === 'string'
    );
  }

  function actionsFrom(value: ActionSlot): Action[] {
    if (Array.isArray(value)) {
      const list: JsonValue[] = value;
      return list.filter(isAction);
    }
    return isAction(value) ? [value] : [];
  }
</script>

<section class="action-list" role="group" aria-label={`${triggerLabel} action list`}>
  <h3>{triggerLabel}</h3>
  {#if actions.length === 0}
    <p class="empty-list">No actions configured.</p>
  {:else}
    <div class="cards">
      {#each actions as action, index (index)}
        <ActionCard
          {action}
          {index}
          count={actions.length}
          {triggerLabel}
          {disabled}
          {onUpdate}
          {onRemove}
          {onMove}
        />
      {/each}
    </div>
  {/if}
  <button
    class="add-action"
    type="button"
    aria-label={`Add ${trigger} action`}
    {disabled}
    onclick={onAdd}
  >+ Add action</button>
</section>

<style>
  .action-list {
    display: grid;
    gap: var(--space-3);
  }

  h3 {
    margin: 0;
    font-size: 0.95rem;
  }

  .cards {
    display: grid;
    gap: var(--space-2);
  }

  .empty-list {
    margin: 0;
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .add-action {
    min-height: 2.5rem;
    border: 1px dashed var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--accent);
    cursor: pointer;
  }

  .add-action:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }
</style>
