<script lang="ts">
  import type { ActionSlot, Button } from '../types';

  export let button: Button;
  export let selected = false;
  export let disabled = false;
  export let onSelect: () => void;

  function actionCount(slot: ActionSlot): number {
    if (Array.isArray(slot)) return slot.length;
    return slot !== null && typeof slot === 'object' ? 1 : 0;
  }
</script>

<button
  class="key-card"
  class:selected
  aria-label={`Key ${button.index + 1}, ${button.label}`}
  aria-pressed={selected}
  {disabled}
  onclick={onSelect}
>
  <span class="key-number">KEY {button.index + 1}</span>
  <span class="key-label">{button.label}</span>
  <span class="trigger-counts" aria-label="Actions per trigger">
    <span class="single">S {actionCount(button.single)}</span>
    <span class="double">D {actionCount(button.double)}</span>
    <span class="hold">H {actionCount(button.hold)}</span>
  </span>
</button>

<style>
  .key-card {
    display: grid;
    min-width: 0;
    min-height: 8.25rem;
    align-content: space-between;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: linear-gradient(145deg, var(--surface-key), var(--surface-raised));
    box-shadow: 0 0.35rem 0 var(--surface-base), 0 0.45rem 0 var(--border);
    text-align: left;
    transition:
      border-color 120ms ease,
      transform 120ms ease;
  }

  .key-card:active:not(:disabled) {
    transform: translateY(0.2rem);
    box-shadow: 0 0.15rem 0 var(--border);
  }

  .key-card.selected {
    border-color: var(--accent);
    box-shadow:
      0 0 0 1px var(--accent),
      0 0.35rem 0 var(--surface-base),
      0 0.45rem 0 var(--accent-strong);
  }

  .key-number {
    color: var(--text-muted);
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.08em;
  }

  .key-label {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .trigger-counts {
    display: flex;
    gap: var(--space-3);
    font-size: 0.75rem;
    font-weight: 700;
  }

  .single {
    color: var(--trigger-single);
  }

  .double {
    color: var(--trigger-double);
  }

  .hold {
    color: var(--trigger-hold);
  }
</style>
