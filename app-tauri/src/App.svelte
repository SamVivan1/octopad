<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import AppShell from './lib/components/AppShell.svelte';
  import { saveConfig } from './lib/api';
  import { refreshAppSnapshot } from './lib/state';
  import { handleAppShortcut } from './lib/shortcuts';

  let shortcutFeedback = '';

  function onKeydown(event: KeyboardEvent) {
    handleAppShortcut(
      event,
      () => void saveFromShortcut(),
      () => void quitFromShortcut()
    );
  }

  async function saveFromShortcut() {
    shortcutFeedback = '';
    try {
      if (!(await saveConfig())) {
        shortcutFeedback = 'Save failed';
        return;
      }
      await refreshAppSnapshot();
      shortcutFeedback = 'Saved';
    } catch (error) {
      shortcutFeedback = `Save failed: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  async function quitFromShortcut() {
    shortcutFeedback = '';
    try {
      await invoke('quit_app_cmd');
    } catch (error) {
      shortcutFeedback = `Quit failed: ${error instanceof Error ? error.message : String(error)}`;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />
<AppShell />
{#if shortcutFeedback}
  <p class="shortcut-feedback" role="status">{shortcutFeedback}</p>
{/if}

<style>
  .shortcut-feedback {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    z-index: 10;
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
  }
</style>
