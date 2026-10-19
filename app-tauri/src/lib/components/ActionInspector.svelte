<script lang="ts">
  /**
   * The inspector of the selected key: its label and its three action lists.
   *
   * Like every other page in M2 this is a view. It reads the selected button
   * from the shared snapshot — which only changes when `octopad-core` emits an
   * event — and it never patches the config itself. Each control reports an
   * intended change through `onEdit`, the page's serialized command queue, and
   * waits for the core to confirm it with `button-changed`.
   *
   * Two M2 rules are visible in the code:
   *
   * - **Nothing is executed.** Editing an action only writes configuration; the
   *   device is never touched from here (no test button, no send).
   * - **Failures are visible.** The shared `lastError` store is rendered as an
   *   alert, so a rejected command is never mistaken for an applied edit.
   */
  import ActionListEditor from './ActionListEditor.svelte';
  import { lastError } from '../state';
  import type { Button, Trigger } from '../types';
  import type { EditQueue } from './ActionCard.svelte';

  interface Props {
    button: Button | null;
    onLabel: (text: string) => void;
    onEdit: EditQueue;
  }

  let { button, onLabel, onEdit }: Props = $props();

  const error = $derived($lastError);

  /** Trigger order and captions are fixed by the firmware, not by the config. */
  const TRIGGERS: ReadonlyArray<{ trigger: Trigger; caption: string }> = [
    { trigger: 'single', caption: 'Single' },
    { trigger: 'double', caption: 'Double' },
    { trigger: 'hold', caption: 'Hold' },
  ];
</script>

<div class="inspector" data-testid="action-inspector">
  {#if error}
    <p class="alert alert--error" role="alert" data-testid="action-error">{error}</p>
  {/if}

  {#if button}
    <div class="inspector__label">
      <label class="inspector__label-text" for="action-label">
        Key label
      </label>
      <input
        id="action-label"
        class="inspector__label-input"
        type="text"
        data-testid="action-label"
        value={button.label ?? ''}
        onchange={(event) => onLabel(event.currentTarget.value)}
      />
    </div>

    <div class="inspector__triggers">
      {#each TRIGGERS as entry (entry.trigger)}
        <ActionListEditor
          trigger={entry.trigger}
          caption={entry.caption}
          slot={button[entry.trigger]}
          {onEdit}
        />
      {/each}
    </div>

    <p class="muted inspector__note">
      Actions run in list order. Editing never changes anything else an action carries, and running
      an action from here arrives with the device integrations.
    </p>
  {:else}
    <p class="muted" data-testid="inspector-empty">Select a key to edit its actions.</p>
  {/if}
</div>

<style>
  .inspector {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .inspector__label {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .inspector__label-text {
    color: var(--text-secondary);
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .inspector__label-input {
    flex: 1;
    min-width: 180px;
    min-height: 30px;
    padding: 4px 10px;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background-color: var(--surface-sunken);
    color: var(--text-primary);
    font-family: inherit;
    font-size: var(--font-size-sm);
  }

  .inspector__triggers {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .inspector__note {
    margin: 0;
    font-size: var(--font-size-xs);
  }
</style>