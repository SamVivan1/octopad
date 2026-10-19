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

<section class="page" data-page="buttons" data-testid="active-page">
  <header class="buttons__header">
    <div class="buttons__heading">
      <h1 class="page__title">Buttons</h1>
      <p class="muted buttons__subtitle">
        {modeLabel} · {buttons.length} keys · editing key {selectedIndex + 1}
      </p>
    </div>

    <ModeSwitch {modes} activeIndex={activeModeIndex} onSelect={selectMode} />
  </header>

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

  <!--
    The inspector slot. It is a scroll container so a long action list never
    stretches the grid, and every edit it makes goes through the same command
    queue as the grid above, so it can never land on a different key than the
    one on screen.
  -->
  <div class="card buttons__inspector" data-testid="buttons-inspector">
    <h2 class="section-title">Key {selectedIndex + 1} — {selectedLabel}</h2>
    <ActionInspector button={selectedButton} onLabel={renameKey} onEdit={runEdit} />
  </div>
</section>

<style>
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

  .buttons__inspector {
    max-height: 420px;
    overflow-y: auto;
  }

  @media (max-width: 640px) {
    .buttons__header {
      align-items: flex-start;
      flex-direction: column;
    }
  }
</style>