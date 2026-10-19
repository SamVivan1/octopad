<script lang="ts">
  /**
   * Desktop / Home Assistant mode switch.
   *
   * The modes come from the config, so this component only decides how they are
   * presented and which one the core currently reports as active. Clicking a
   * mode asks the caller to switch; the highlight only moves once the core has
   * emitted `mode-changed`, so a rejected switch leaves the UI where it was.
   */
  import type { Mode } from '../types';

  interface Props {
    modes: Mode[];
    activeIndex: number;
    busy?: boolean;
    onSelect: (index: number) => void;
  }

  let { modes, activeIndex, busy = false, onSelect }: Props = $props();

  function modeName(mode: Mode, index: number): string {
    const name = mode.name?.trim();
    if (name) return name;
    return mode.id?.trim() || `Mode ${index + 1}`;
  }
</script>

<div class="mode-switch" role="group" aria-label="Mode" data-testid="mode-switch">
  {#each modes as mode, index (mode.id || index)}
    {@const active = index === activeIndex}
    <button
      type="button"
      class="btn mode-switch__mode"
      class:is-active={active}
      aria-pressed={active}
      data-testid="mode-button"
      data-mode-id={mode.id}
      data-mode-index={index}
      disabled={busy}
      onclick={() => onSelect(index)}
    >
      {modeName(mode, index)}
    </button>
  {/each}
</div>

<style>
  .mode-switch {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: 3px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    background-color: var(--surface-inset);
  }

  .mode-switch__mode {
    border-color: transparent;
    background-color: transparent;
    color: var(--text-secondary);
  }

  .mode-switch__mode:hover:not(:disabled) {
    color: var(--text-primary);
  }

  .mode-switch__mode.is-active {
    border-color: var(--accent-soft);
    background-color: var(--accent-soft);
    color: var(--accent);
  }
</style>