<script lang="ts">
  /**
   * The eight physical keys as a responsive grid.
   *
   * The layout numbers live in custom properties on the grid element so the
   * stylesheet and the tests read the same contract:
   *
   * - `--key-grid-columns` — how many columns a wide *container* shows (4, i.e.
   *   the 4x2 grid of the physical device).
   * - `--key-grid-min-columns` — the floor, which the narrow layout honours.
   * - `--key-grid-min-track` — the smallest a track may get, capped at a share
   *   of the container so {@link MIN_COLUMNS} of them always fit.
   *
   * The element is also a query container, so the track count follows the width
   * of the column the grid is in. That matters now the Buttons page is split:
   * the left column is narrower than the window, and a viewport-driven
   * breakpoint would squeeze eight keys into a column sized for four. Where
   * container queries are unavailable the base rule still stands, which is the
   * narrower `auto-fit` layout with the two-column floor — usable, and never
   * below two columns.
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

  /** Wide-container layout: 4 columns x 2 rows for the eight keys. */
  const COLUMNS = 4;
  /** The grid must never collapse to a single column in a narrow column. */
  const MIN_COLUMNS = 2;
  /** Widest a keycap may get, so eight of them still fill the column. */
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
  <div class="key-grid__keys">
    {#each buttons as button (button.index)}
      <KeyCard {button} selected={button.index === selectedIndex} {onSelect} />
    {/each}
  </div>
</div>

<style>
  /*
   * The host carries the published layout contract and is the query container;
   * the tracks live on the inner element, because a container query can only
   * style descendants of the container it measures.
   */
  .key-grid {
    container-type: inline-size;
  }

  .key-grid__keys {
    display: grid;
    gap: var(--space-4);
    /* Narrow containers: as many columns as fit, never fewer than
       `--key-grid-min-columns` thanks to the share-capped minimum track. */
    grid-template-columns: repeat(auto-fit, minmax(var(--key-grid-min-track), 1fr));
  }

  /*
   * Wide containers — the physical 4x2 layout, whatever the window is doing.
   * 520px is about four comfortable keycaps plus their gaps, so a narrower
   * column keeps the `auto-fit` layout above.
   */
  @container (min-width: 520px) {
    .key-grid__keys {
      grid-template-columns: repeat(var(--key-grid-columns), minmax(0, 1fr));
    }
  }

  @container (max-width: 400px) {
    .key-grid__keys {
      gap: var(--space-3);
    }
  }
</style>