<script lang="ts">
  /**
   * The action list of one trigger — single, double or hold.
   *
   * A trigger slot has two shapes on disk: a legacy single object or a list of
   * actions (and, for hand-written configs, neither). This component only ever
   * *reads* the slot through the `actions_list` view and sends commands; it
   * never rewrites the slot itself, so the on-disk shape is decided by the core
   * exactly as before (`TriggerActions::from_list` collapses a single action
   * back to the legacy object). That is why editing an action of a legacy slot
   * and editing one entry of a list slot use the same code path here.
   *
   * A slot the core preserved verbatim — neither object nor list — is shown as
   * read-only, because guessing at its contents would rewrite it.
   */
  import ActionCard from './ActionCard.svelte';
  import * as api from '../api';
  import { runCommand } from '../state';
  import { show } from '../toast';
  import type { Action, Trigger, TriggerSlot } from '../types';
  import type { EditQueue } from './ActionCard.svelte';

  interface Props {
    trigger: Trigger;
    caption: string;
    /** Which physical key this slot belongs to; the core must be told. */
    buttonIndex: number;
    slot: TriggerSlot;
    onEdit: EditQueue;
  }

  let { trigger, caption, buttonIndex, slot, onEdit }: Props = $props();

  function isAction(value: unknown): value is Action {
    return (
      typeof value === 'object' &&
      value !== null &&
      !Array.isArray(value) &&
      typeof (value as Action).type === 'string'
    );
  }

  /** `TriggerActions::as_slice`: an object is one action, a list is its entries. */
  const actions = $derived(
    Array.isArray(slot) ? slot.filter(isAction) : isAction(slot) ? [slot] : []
  );

  /** A slot that is neither an action nor a list is preserved, never rewritten. */
  const unreadable = $derived(actions.length === 0 && !Array.isArray(slot));

  function add(): void {
    void onEdit(() => runCommand(`add ${caption} action`, () => api.addAction(trigger)));
  }

  /**
   * "Test all" — run every runnable action of this trigger and report as one
   * joined toast, exactly the old app's `_test_all`: "Test: " + reasons joined
   * with " | ", green only when every outcome is ok, and the info toast when
   * nothing here can run at all (no invoke either).
   *
   * Like the card's Test button this is execution, not an edit: it bypasses
   * `onEdit` so it never queues behind a pending edit. A failed invoke is not
   * an outcome — `runCommand` records it in `lastError` instead of a toast.
   */
  async function testAll(): Promise<void> {
    const runnable = actions.filter((action) => action.type !== 'none');
    if (runnable.length === 0) {
      show('No actions on this trigger', 'info');
      return;
    }
    try {
      const outcomes = await api.testTrigger(buttonIndex, trigger);
      const ok = outcomes.every((outcome) => outcome.ok);
      const message = outcomes.map((outcome) => outcome.reason).join(' | ');
      show(`Test: ${message}`, ok ? 'success' : 'error');
    } catch (error) {
      void runCommand(`test all ${caption} actions`, () => Promise.reject(error));
    }
  }
</script>

<section
  class="trigger"
  data-testid="trigger-list"
  data-trigger={trigger}
  aria-label="{caption} actions"
>
  <header class="trigger__head">
    <h3 class="trigger__title">
      <span class="trigger__dot" aria-hidden="true"></span>
      {caption}
    </h3>
    <span class="pill trigger__count">{actions.length}</span>
    <button
      type="button"
      class="btn trigger__test"
      data-testid="action-test-all"
      title="Test every action on this trigger"
      onclick={testAll}
    >
      Test all
    </button>
    <button
      type="button"
      class="btn trigger__add"
      data-testid="action-add"
      title="Add an action to this trigger"
      onclick={add}
    >
      Add action
    </button>
  </header>

  {#if unreadable}
    <p class="muted trigger__note" data-testid="trigger-unreadable">
      This trigger holds an unsupported value, so it is shown unchanged and no action command is
      sent. Adding an action replaces it with a normal action list.
    </p>
  {/if}

  <div class="trigger__actions" data-testid="action-list">
    {#each actions as action, index (index)}
      <ActionCard
        {trigger}
        triggerCaption={caption}
        {index}
        total={actions.length}
        {action}
        {onEdit}
      />
    {/each}
  </div>
</section>

<style>
  .trigger {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background-color: var(--surface-inset);
  }

  .trigger__head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .trigger__title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  /* The trigger identity comes from the design system, never per-OS. */
  .trigger[data-trigger='single'] .trigger__dot {
    background-color: var(--trigger-single);
  }

  .trigger[data-trigger='double'] .trigger__dot {
    background-color: var(--trigger-double);
  }

  .trigger[data-trigger='hold'] .trigger__dot {
    background-color: var(--trigger-hold);
  }

  .trigger__dot {
    width: 7px;
    height: 7px;
    border-radius: var(--radius-pill);
  }

  .trigger__count {
    padding: 0 8px;
    font-size: var(--font-size-xs);
  }

  .trigger__add {
    /* Last in the header: its auto margin pushes both end buttons right. */
    margin-left: auto;
  }

  .trigger__note {
    margin: 0;
    font-size: var(--font-size-xs);
  }

  .trigger__actions {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .trigger__actions:empty {
    display: none;
  }
</style>