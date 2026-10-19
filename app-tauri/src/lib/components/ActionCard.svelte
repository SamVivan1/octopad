<script lang="ts">
  /**
   * One action inside a trigger slot: its type, its fields, and the controls
   * that change its position in the list.
   *
   * Every control here issues one command through the page's serialized
   * `onEdit` queue and then stops: the card renders whatever the core last
   * reported. Nothing is patched locally, so a rejected command simply leaves
   * the previous action on screen (with the failure surfaced by the inspector).
   *
   * Actions are edited only. Running one is an M3 feature, so there is no test
   * button here — nothing in this component can reach the device.
   */
  import ActionFields, { ACTION_TYPES, actionTypeLabel, withType } from './ActionFields.svelte';
  import * as api from '../api';
  import { runCommand } from '../state';
  import type { Action, Trigger } from '../types';

  /** The page's command queue: one edit at a time, in the order it was queued. */
  export type EditQueue = <T>(command: () => Promise<T>) => Promise<T>;

  interface Props {
    trigger: Trigger;
    triggerCaption: string;
    index: number;
    total: number;
    action: Action;
    onEdit: EditQueue;
  }

  let { trigger, triggerCaption, index, total, action, onEdit }: Props = $props();

  const label = $derived(actionTypeLabel(action.type));
  const canMoveUp = $derived(index > 0);
  const canMoveDown = $derived(index < total - 1);
  /** The core keeps one action per trigger, so the last one cannot be removed. */
  const canRemove = $derived(total > 1);

  const position = $derived(`${triggerCaption} action ${index + 1}`);

  function setAction(next: Action): Promise<unknown> {
    return onEdit(() =>
      runCommand(`set ${position} (${label})`, () => api.setAction(trigger, index, next))
    );
  }

  function changeType(type: string): void {
    void setAction(withType(action, type));
  }

  function changeField(next: Action): void {
    void setAction(next);
  }

  function moveUp(): void {
    if (!canMoveUp) return;
    void onEdit(() =>
      runCommand(`move ${position} up`, () => api.moveAction(trigger, index, -1))
    );
  }

  function moveDown(): void {
    if (!canMoveDown) return;
    void onEdit(() =>
      runCommand(`move ${position} down`, () => api.moveAction(trigger, index, 1))
    );
  }

  function remove(): void {
    // The core always keeps one action per trigger. The control is disabled as
    // well, but a programmatic click must not slip a rejected edit past it.
    if (!canRemove) return;
    void onEdit(() => runCommand(`remove ${position}`, () => api.removeAction(trigger, index)));
  }
</script>

<article
  class="action-card"
  data-testid="action-card"
  data-trigger={trigger}
  data-index={index}
  data-type={action.type}
  data-position={index + 1}
>
  <header class="action-card__head">
    <span class="action-card__position" data-testid="action-position">{index + 1}</span>

    <select
      class="action-card__type"
      aria-label="Type of {triggerCaption} action {index + 1}"
      data-testid="action-type"
      value={action.type}
      onchange={(event) => changeType(event.currentTarget.value)}
    >
      {#each ACTION_TYPES as spec (spec.type)}
        <option value={spec.type}>{spec.label}</option>
      {/each}
      {#if !ACTION_TYPES.some((spec) => spec.type === action.type)}
        <option value={action.type}>{action.type}</option>
      {/if}
    </select>

    <span class="action-card__controls">
      <button
        type="button"
        class="btn action-card__control"
        data-testid="action-move-up"
        disabled={!canMoveUp}
        title="Move up"
        onclick={moveUp}
      >
        ↑
      </button>
      <button
        type="button"
        class="btn action-card__control"
        data-testid="action-move-down"
        disabled={!canMoveDown}
        title="Move down"
        onclick={moveDown}
      >
        ↓
      </button>
      <button
        type="button"
        class="btn action-card__control"
        data-testid="action-remove"
        disabled={!canRemove}
        title={canRemove ? 'Remove this action' : 'A trigger always keeps one action'}
        onclick={remove}
      >
        ✕
      </button>
    </span>
  </header>

  <ActionFields
    {action}
    idPrefix="action-{trigger}-{index}"
    onChange={changeField}
  />
</article>

<style>
  .action-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background-color: var(--surface-inset);
  }

  .action-card__head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .action-card__position {
    flex: none;
    min-width: 20px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: var(--font-size-xs);
    font-weight: 700;
    text-align: center;
  }

  .action-card__type {
    flex: 1;
    min-width: 120px;
    min-height: 28px;
    padding: 2px 8px;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    background-color: var(--surface-sunken);
    color: var(--text-primary);
    font-family: inherit;
    font-size: var(--font-size-sm);
    font-weight: 600;
  }

  .action-card__controls {
    display: flex;
    gap: var(--space-1);
  }

  .action-card__control {
    min-height: 26px;
    min-width: 30px;
    padding: 2px 8px;
  }
</style>