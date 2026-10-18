<script lang="ts">
  import type { Action, TriggerName } from '../types';
  import { appState, refreshAppSnapshot } from '../state';
  import {
    addAction,
    moveAction,
    removeAction,
    selectButton,
    setAction,
    setLabel,
    setMode
  } from '../api';
  import ActionInspector from '../components/ActionInspector.svelte';
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

  async function mutateActions(label: string, operation: () => Promise<void>) {
    if (interactionBusy) return;
    interactionBusy = true;
    feedback = '';
    try {
      await operation();
      await refreshAppSnapshot();
    } catch (error) {
      feedback = `${label} failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      interactionBusy = false;
    }
  }

  function updateAction(trigger: TriggerName, index: number, action: Action) {
    return mutateActions('Action update', () => setAction(trigger, index, action));
  }

  function addTriggerAction(trigger: TriggerName) {
    return mutateActions('Add action', () => addAction(trigger));
  }

  function removeTriggerAction(trigger: TriggerName, index: number) {
    return mutateActions('Remove action', () => removeAction(trigger, index));
  }

  function moveTriggerAction(trigger: TriggerName, index: number, delta: number) {
    return mutateActions('Reorder action', () => moveAction(trigger, index, delta));
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

      {#if selectedButton}
        <ActionInspector
          button={selectedButton}
          {labelDraft}
          disabled={interactionBusy}
          onLabelChange={updateLabel}
          onActionUpdate={updateAction}
          onActionAdd={addTriggerAction}
          onActionRemove={removeTriggerAction}
          onActionMove={moveTriggerAction}
        />
      {:else}
        <aside class="empty-inspector" aria-label="Selected key editor">
          <h2>Selected key</h2>
          <p>No key is selected.</p>
        </aside>
      {/if}
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
  .empty-state {
    margin: 0;
    color: var(--text-muted);
  }

  .editor-layout {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) minmax(15rem, 0.8fr);
    align-items: start;
    gap: var(--space-5);
  }

  .feedback {
    color: var(--error);
  }

  .empty-inspector {
    display: grid;
    align-content: start;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-raised);
  }

  .empty-inspector h2,
  .empty-inspector p {
    margin: 0;
  }

  .empty-inspector p {
    color: var(--text-muted);
  }

  @media (max-width: 900px) {
    .editor-layout {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
