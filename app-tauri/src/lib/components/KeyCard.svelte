<script lang="ts">
  /**
   * One physical keycap.
   *
   * The card shows what the core reported for this button and nothing else: the
   * physical key number (its position on the device, always 1-8), the label, and
   * a single/double/hold indicator that is lit when the trigger holds an action
   * other than `none`.
   *
   * Trigger slots come in three shapes on disk — a legacy single object, a list
   * of actions, or a value this UI does not understand — so the indicator only
   * ever *reads* the slot and never rewrites it.
   */
  import type { Action, Button, Trigger, TriggerSlot } from '../types';

  interface Props {
    button: Button;
    selected: boolean;
    onSelect: (index: number) => void;
  }

  let { button, selected, onSelect }: Props = $props();

  /** Trigger order and captions are fixed by the firmware, not by the config. */
  const TRIGGERS: ReadonlyArray<{ trigger: Trigger; caption: string }> = [
    { trigger: 'single', caption: 'Single' },
    { trigger: 'double', caption: 'Double' },
    { trigger: 'hold', caption: 'Hold' },
  ];

  function isAction(value: unknown): value is Action {
    return (
      typeof value === 'object' &&
      value !== null &&
      !Array.isArray(value) &&
      typeof (value as Action).type === 'string'
    );
  }

  /** Never throws and never mutates: an unknown slot simply has no actions. */
  function triggerActions(slot: TriggerSlot): Action[] {
    if (Array.isArray(slot)) return slot.filter(isAction);
    return isAction(slot) ? [slot] : [];
  }

  const keyNumber = $derived(button.index + 1);
  const label = $derived(button.label?.trim() ?? '');

  const indicators = $derived(
    TRIGGERS.map(({ trigger, caption }) => {
      const actions = triggerActions(button[trigger]);
      const active = actions.some((action) => action.type !== 'none');
      return {
        trigger,
        caption,
        active,
        summary: active
          ? actions
              .map((action) => action.type)
              .filter((type) => type !== 'none')
              .join(', ')
          : `${caption}: no action`,
      };
    })
  );
</script>

<button
  type="button"
  class="key-card"
  class:is-selected={selected}
  aria-pressed={selected}
  aria-label="Key {keyNumber}: {label || 'Unlabelled'}"
  data-testid="key-card"
  data-key={button.index}
  data-selected={selected}
  onclick={() => onSelect(button.index)}
>
  <span class="key-card__top">
    <span class="key-card__number" data-testid="key-card-number">{keyNumber}</span>
    <span class="key-card__label" data-testid="key-card-label" title={label}>
      {label || 'Unlabelled'}
    </span>
  </span>

  <span class="key-card__triggers">
    {#each indicators as indicator (indicator.trigger)}
      <span
        class="pill key-card__trigger"
        data-trigger={indicator.trigger}
        data-active={indicator.active}
        data-testid="key-card-trigger"
        title={indicator.summary}
      >
        <span class="pill__dot" aria-hidden="true"></span>
        {indicator.caption}
      </span>
    {/each}
  </span>
</button>

<style>
  /* Keycap: a raised surface with a hard bottom edge, echoing the device. */
  .key-card {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    gap: var(--space-3);
    width: 100%;
    min-height: 74px;
    padding: var(--space-4);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    background-color: var(--surface-raised);
    box-shadow: inset 0 -3px 0 var(--border-subtle), var(--shadow-raised);
    color: inherit;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      border-color 120ms ease;
  }

  .key-card:hover {
    background-color: var(--surface-overlay);
  }

  .key-card.is-selected {
    border-color: var(--accent);
    background-color: var(--accent-softer);
    box-shadow: inset 0 -3px 0 var(--accent), var(--shadow-raised);
  }

  .key-card__top {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    min-width: 0;
  }

  .key-card__number {
    flex: none;
    min-width: 22px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: var(--font-size-sm);
    font-weight: 700;
    text-align: center;
  }

  .key-card.is-selected .key-card__number {
    color: var(--accent);
  }

  .key-card__label {
    min-width: 0;
    overflow: hidden;
    font-size: var(--font-size-md);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .key-card__triggers {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  /* Trigger identity is the one defined in the design system: single = accent,
     double = success, hold = warning. An unconfigured trigger stays faint. */
  .key-card__trigger {
    padding: 0 8px;
    font-size: var(--font-size-xs);
  }

  .key-card__trigger[data-trigger='single'] {
    color: var(--trigger-single);
  }

  .key-card__trigger[data-trigger='double'] {
    color: var(--trigger-double);
  }

  .key-card__trigger[data-trigger='hold'] {
    color: var(--trigger-hold);
  }

  .key-card__trigger[data-active='false'] {
    border-color: var(--border-subtle);
    background-color: transparent;
    color: var(--text-faint);
  }
</style>