<script lang="ts">
  import type { Mode } from '../types';

  export let modes: Mode[];
  export let activeModeIndex: number;
  export let disabled = false;
  export let onSelect: (index: number) => void;

  function label(mode: Mode, index: number): string {
    if (mode.id === 'ha') return 'Home Assistant';
    if (mode.id === 'desktop') return 'Desktop';
    return mode.name || `Mode ${index + 1}`;
  }
</script>

<div class="mode-switch" role="group" aria-label="Button mode">
  {#each modes as mode, index (mode.id)}
    <button
      class:active={activeModeIndex === index}
      aria-pressed={activeModeIndex === index}
      {disabled}
      onclick={() => onSelect(index)}
    >
      {label(mode, index)}
    </button>
  {/each}
</div>

<style>
  .mode-switch {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: var(--space-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-raised);
  }

  button {
    min-height: 2.5rem;
    padding: 0 var(--space-4);
    border-color: transparent;
    background: transparent;
    color: var(--text-muted);
  }

  button.active {
    border-color: var(--accent-strong);
    background: var(--accent-strong);
    color: #fff;
  }
</style>
