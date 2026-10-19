<script lang="ts">
  /**
   * The eight physical keys as a responsive grid.
   *
   * The layout numbers live in custom properties on the grid element so the
   * stylesheet and the tests read the same contract:
   *
   * - `--key-grid-columns` — how many columns a wide window shows (4, i.e. the
   *   4×2 grid of the physical device).
   * - `--key-grid-min-columns` — the floor, which the narrow layout honours.
   * - `--key-grid-min-track` — the smallest a track may get, capped at a share
   *   of the container so {@link MIN_COLUMNS} of them always fit.
   *
   * The component is pure presentation: it reports clicks through `onSelect`
   * and never touches the config or the selection itself.
   */
  import KeyCard from './KeyCard.svelte';
  import type { Button } from '../types';

  interface Props {
    buttons: Button[];
    selectedIndex: number;
    onSelect: (index: number) => void;
  }

  let { buttons, selectedIndex, onSelect }: Props = $props();

  /** Wide-window layout: 4 columns × 2 rows for the eight keys. */
  const COLUMNS = 4;
  /** The grid must never collapse to a single column in a narrow window. */
  const MIN_COLUMNS = 2;
  /** Widest a keycap may get, so eight of them still fill the window. */
  const MAX_TRACK = '168px';
  /** Share of the container one track may claim while `MIN_COLUMNS` still fit. */
  const MAX_TRACK_SHARE = Math.floor(90 / MIN_COLUMNS);

  const MIN_TRACK = `min(${MAX_TRACK_SHARE}%, ${MAX_TRACK})`;
</script>

<div
  class="key-grid"
  data-testid="key-grid"
  data-columns={COLUMNS}
  data-min-columns={MIN_COLUMNS}
  style="--key-grid-columns: {COLUMNS}; --key-grid-min-columns: {MIN_COLUMNS}; --key-grid-min-track: {MIN_TRACK};"
>
  {#each buttons as button (button.index)}
    <KeyCard {button} selected={button.index === selectedIndex} {onSelect} />
  {/each}
</div>

<style>
  .key-grid {
    display: grid;
    gap: var(--space-4);
    /* Narrow windows: as many columns as fit, never fewer than
       `--key-grid-min-columns` thanks to the share-capped minimum track. */
    grid-template-columns: repeat(auto-fit, minmax(var(--key-grid-min-track), 1fr));
  }

  @media (min-width: 760px) {
    /* Wide windows: the physical 4×2 layout. */
    .key-grid {
      grid-template-columns: repeat(var(--key-grid-columns), minmax(0, 1fr));
    }
  }

  @media (max-width: 520px) {
    .key-grid {
      gap: var(--space-3);
    }
  }
</style>