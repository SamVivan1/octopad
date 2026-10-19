<script lang="ts">
  /**
   * Buttons page: mode switch, the eight physical keys, and the inspector area
   * the action editor fills in next.
   *
   * Two rules shape the implementation (M2 Review Focus — "rapid mode/button
   * selection"):
   *
   * 1. **The page holds no config of its own.** Every rendered value — mode,
   *    selection, labels, trigger indicators — is read from the shared `appState`
   *    snapshot, which only changes when `octopad-core` emits an event. A click
   *    therefore issues a command and waits; the highlight moves when the core
   *    confirms with `selection-changed` / `mode-changed`, never before.
   *
   * 2. **Commands are serialised.** Card clicks, mode switches and every future
   *    edit go through one queue, so a fast click cannot route an edit to the
   *    previously selected key: the edit waits for the in-flight selection.
   */
  import ActionInspector from '../components/ActionInspector.svelte';
  import KeyGrid from '../components/KeyGrid.svelte';
  import ModeSwitch from '../components/ModeSwitch.svelte';
  import * as api from '../api';
  import { runEdit as enqueueEdit } from '../editQueue';
  import { appState, runCommand } from '../state';

  const snapshot = $derived($appState);

  const modes = $derived(snapshot?.config.modes ?? []);
  const activeModeIndex = $derived(snapshot?.modeIndex ?? 0);
  const mode = $derived(modes[activeModeIndex] ?? null);
  const modeLabel = $derived(mode ? (mode.name?.trim() || mode.id) : 'no mode');
  const buttons = $derived(mode?.buttons ?? []);
  const selectedIndex = $derived(snapshot?.selectedIndex ?? 0);
  const selectedButton = $derived(
    buttons.find((button) => button.index === selectedIndex) ?? buttons[0] ?? null
  );
  const selectedLabel = $derived(selectedButton?.label?.trim() || 'Unlabelled');

  // -- the two-column layout ---------------------------------------------------
  //
  // The intended page is buttons left, actions right; M2 stacked them and the
  // owner found it by running the app. The numbers below are the contract the
  // stylesheet and the tests both read — `SPLIT_COLUMNS` columns while the
  // window is at least `SPLIT_BREAKPOINT_PX` wide, one column below it. The
  // tracks, the gap and the per-column scroll bound come from the shared
  // `--shell-*` layout tokens in `app.css`, never from a local rule.
  const SPLIT_COLUMNS = 2;
  const SPLIT_BREAKPOINT_PX = 900;

  // -- the command queue -------------------------------------------------------
  //
  // One app-wide queue, shared with every other page (see `../editQueue`), so a
  // command issued here can never overtake an edit another page already queued.

  async function selectKey(index: number): Promise<void> {
    await enqueueEdit(() => runCommand(`select key ${index + 1}`, () => api.selectButton(index)));
  }

  async function selectMode(index: number): Promise<void> {
    await enqueueEdit(() => runCommand('switch mode', () => api.setMode(index)));
  }

  /**
   * The only way edits reach the core.
   *
   * The action editor and the label editor call this instead of `api` directly,
   * which is what guarantees their command is issued *after* any selection the
   * user triggered a moment earlier.
   */
  export async function runEdit<T>(command: () => Promise<T>): Promise<T> {
    return enqueueEdit(command);
  }

  /** Rename the key on screen, through the same queue as every other edit. */
  async function renameKey(text: string): Promise<void> {
    await enqueueEdit(() => runCommand('set the key label', () => api.setLabel(text)));
  }
</script>

<section class="page buttons__page" data-page="buttons" data-testid="active-page">
  <header class="buttons__header">
    <div class="buttons__heading">
      <h1 class="page__title">Buttons</h1>
      <p class="muted buttons__subtitle">
        {modeLabel} · {buttons.length} keys · editing key {selectedIndex + 1}
      </p>
    </div>

    <ModeSwitch {modes} activeIndex={activeModeIndex} onSelect={selectMode} />
  </header>

  <!--
    The split region: buttons on the left, actions on the right. Both panels are
    always rendered — the grid card and the inspector card — and the grid comes
    first in the DOM, so the reading order matches the visual order even when the
    region collapses to a single column below the breakpoint.

    The layout intent is published as data (`data-layout`, `data-columns`,
    `data-collapse-below`) and the measurements come from the shared `--shell-*`
    tokens, so `ButtonsPage.test.ts` can pin the contract without asserting
    pixels.
  -->
  <div
    class="buttons__split"
    data-testid="buttons-layout"
    data-layout="split"
    data-columns={SPLIT_COLUMNS}
    data-collapse-below={`${SPLIT_BREAKPOINT_PX}px`}
    style="--split-grid: var(--shell-split-grid); --split-inspector: var(--shell-split-inspector); --split-gap: var(--shell-split-gap); --split-column-height: var(--shell-split-column-height);"
  >
    <div class="buttons__column buttons__column--grid" data-column="grid">
      <div class="card">
        <h2 class="section-title">Key grid</h2>

        {#if buttons.length > 0}
          <KeyGrid {buttons} {selectedIndex} onSelect={selectKey} />
        {:else}
          <p class="muted" data-testid="buttons-empty">
            This mode has no keys configured yet.
          </p>
        {/if}
      </div>
    </div>

    <!--
      The inspector slot. It scrolls inside its own column so a long action list
      never stretches the grid next to it, and every edit it makes goes through
      the same command queue as the grid, so it can never land on a different key
      than the one on screen.
    -->
    <div class="buttons__column buttons__column--inspector" data-column="inspector">
      <div class="card buttons__inspector" data-testid="buttons-inspector">
        <h2 class="section-title">Key {selectedIndex + 1} — {selectedLabel}</h2>
        <ActionInspector button={selectedButton} onLabel={renameKey} onEdit={runEdit} />
      </div>
    </div>
  </div>
</section>

<style>
  .buttons__page {
    max-width: var(--shell-split-page-max);
  }

  .buttons__header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .buttons__heading {
    min-width: 0;
  }

  .buttons__subtitle {
    margin: 2px 0 0;
    font-size: var(--font-size-sm);
  }

  /* The split itself: two columns while the window is wide enough. */
  .buttons__split {
    display: grid;
    grid-template-columns: var(--split-grid) var(--split-inspector);
    gap: var(--split-gap);
    align-items: start;
  }

  /*
   * Each column scrolls on its own rather than letting its content grow, so the
   * eight keys stay put while a long action list is scrolled through, and the
   * page itself never has to scroll in the wide layout.
   */
  .buttons__column {
    min-width: 0;
    max-height: var(--split-column-height);
    overflow-y: auto;
  }

  /*
   * Below the split breakpoint (the literal mirrors `--shell-split-breakpoint`
   * in `app.css`, which a media query cannot read) the region collapses to one
   * column: the grid first, the inspector under it. Each column keeps its own
   * scroll bound, so the inspector stays usable instead of stretching the page.
   */
  @media (max-width: 900px) {
    .buttons__split {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  @media (max-width: 640px) {
    .buttons__header {
      align-items: flex-start;
      flex-direction: column;
    }
  }
</style>