<script lang="ts">
  import { appState, refreshAppSnapshot } from '../state';
  import { selectButton, setLabel, setMode } from '../api';
  import KeyGrid from '../components/KeyGrid.svelte';
  import ModeSwitch from '../components/ModeSwitch.svelte';

  let interactionBusy = false;
  let labelDraft = '';
  let feedback = '';

  $: snapshot = $appState.snapshot;
  $: mode = snapshot?.config.modes[snapshot.modeIndex];
  $: selectedButton = mode?.buttons[snapshot?.selectedIndex ?? -1];
  $: if (!interactionBusy) labelDraft = selectedButton?.label ?? '';

  async function changeMode(index: number) {
    if (interactionBusy) return;
    interactionBusy = true;
    feedback = '';
    try {
      if (!(await setMode(index))) {
        feedback = 'Could not change mode';
        return;
      }
      await refreshAppSnapshot();
    } catch (error) {
      feedback = `Mode change failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      interactionBusy = false;
    }
  }

  async function selectKey(index: number) {
    if (interactionBusy) return;
    interactionBusy = true;
    feedback = '';
    try {
      if (!(await selectButton(index))) {
        feedback = 'Could not select key';
        return;
      }
      await refreshAppSnapshot();
    } catch (error) {
      feedback = `Key selection failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      interactionBusy = false;
    }
  }

  async function updateLabel(value: string) {
    if (interactionBusy || !selectedButton || value === selectedButton.label) return;
    interactionBusy = true;
    labelDraft = value;
    feedback = '';
    try {
      await setLabel(labelDraft);
      await refreshAppSnapshot();
    } catch (error) {
      feedback = `Label update failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      interactionBusy = false;
    }
  }
</script>

<section class="buttons-page" aria-labelledby="buttons-title">
  <div class="page-heading">
    <div>
      <h1 id="buttons-title">Buttons</h1>
      <p>Select a keycap to edit its label and actions.</p>
    </div>
    {#if snapshot}
      <ModeSwitch
        modes={snapshot.config.modes}
        activeModeIndex={snapshot.modeIndex}
        disabled={interactionBusy}
        onSelect={changeMode}
      />
    {/if}
  </div>

  {#if feedback}
    <p class="feedback" role="alert">{feedback}</p>
  {/if}

  {#if !mode}
    <p class="empty-state">Loading button configuration…</p>
  {:else}
    <div class="editor-layout">
      <KeyGrid
        buttons={mode.buttons}
        modeName={mode.name}
        selectedIndex={snapshot?.selectedIndex ?? 0}
        disabled={interactionBusy}
        onSelect={selectKey}
      />

      <aside class="inspector" aria-label="Selected key editor">
        <h2>Key {selectedButton ? selectedButton.index + 1 : '—'}</h2>
        {#if selectedButton}
          <label for="key-label">Label</label>
          <input
            id="key-label"
            aria-label="Current key label"
            value={labelDraft}
            disabled={interactionBusy}
            onchange={(event) => updateLabel(event.currentTarget.value)}
          />
          <div class="trigger-summary">
            <span>Single</span>
            <span>Double</span>
            <span>Hold</span>
          </div>
        {:else}
          <p>No key is selected.</p>
        {/if}
      </aside>
    </div>
  {/if}
</section>

<style>
  .page-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-4);
    margin-bottom: var(--space-5);
  }

  h1 {
    margin: 0 0 var(--space-1);
    font-size: 1.5rem;
  }

  .page-heading p,
  .empty-state,
  .inspector p {
    margin: 0;
    color: var(--text-muted);
  }

  .editor-layout {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) minmax(15rem, 0.8fr);
    align-items: start;
    gap: var(--space-5);
  }

  .inspector {
    position: sticky;
    top: var(--space-4);
    display: grid;
    max-height: calc(100vh - 7rem);
    gap: var(--space-3);
    overflow-y: auto;
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-raised);
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  label {
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  input {
    min-width: 0;
    min-height: 2.5rem;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-base);
  }

  .trigger-summary {
    display: grid;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  .trigger-summary span {
    padding: var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--surface-key);
  }

  .trigger-summary span:nth-child(1) {
    color: var(--trigger-single);
  }

  .trigger-summary span:nth-child(2) {
    color: var(--trigger-double);
  }

  .trigger-summary span:nth-child(3) {
    color: var(--trigger-hold);
  }

  .feedback {
    color: var(--error);
  }

  @media (max-width: 900px) {
    .editor-layout {
      grid-template-columns: minmax(0, 1fr);
    }

    .inspector {
      position: static;
      max-height: none;
    }
  }
</style>
