<script lang="ts">
  import KeyCard from './KeyCard.svelte';
  import type { Button } from '../types';

  export let buttons: Button[];
  export let modeName: string;
  export let selectedIndex: number;
  export let disabled = false;
  export let onSelect: (index: number) => void;
</script>

<div class="key-grid" role="group" aria-label={`${modeName} buttons`}>
  {#each buttons as button (button.index)}
    <KeyCard
      {button}
      selected={button.index === selectedIndex}
      {disabled}
      onSelect={() => onSelect(button.index)}
    />
  {/each}
</div>

<style>
  .key-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--space-3);
  }

  @media (max-width: 720px) {
    .key-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--space-2);
    }
  }
</style>
