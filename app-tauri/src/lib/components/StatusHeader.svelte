<script lang="ts">
  import { appState, refreshAppSnapshot } from '../state';
  import { saveConfig } from '../api';

  let saveFeedback = '';
  let saving = false;

  async function save() {
    saving = true;
    saveFeedback = '';
    try {
      const saved = await saveConfig();
      if (!saved) {
        saveFeedback = 'Save failed';
        return;
      }
      saveFeedback = 'Saved';
      try {
        await refreshAppSnapshot();
      } catch {
        saveFeedback = 'Saved; current state could not be refreshed';
      }
    } catch (error) {
      saveFeedback = `Save failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      saving = false;
    }
  }
</script>

<header class="status-header">
  <div class="brand">
    <span class="brand-mark" aria-hidden="true">O</span>
    <span>OctoPad</span>
  </div>

  <div class="device-status" aria-label="Device status">
    <span class:connected={$appState.snapshot?.serial.connected} class="status-pill">
      {$appState.snapshot?.serial.connected ? 'Serial connected' : 'Serial disconnected'}
    </span>
    <span class:connected={$appState.snapshot?.ble.connected} class="status-pill">
      {$appState.snapshot?.ble.connected ? 'BLE connected' : 'BLE disconnected'}
    </span>
  </div>

  <div class="device-info">
    <span>{$appState.snapshot?.device.name ?? 'No device'}</span>
    <span class="muted">
      {$appState.snapshot?.device.firmwareVersion
        ? `Firmware v${$appState.snapshot.device.firmwareVersion}`
        : 'Firmware unknown'}
    </span>
  </div>

  <div class="save-area">
    <span class:dirty={$appState.snapshot?.dirty} class="save-state">
      {$appState.snapshot?.dirty ? 'Unsaved changes' : 'All changes saved'}
    </span>
    <button onclick={save} disabled={saving || !$appState.snapshot?.dirty}>Save</button>
    {#if saveFeedback}
      <span class="save-feedback" role="status">{saveFeedback}</span>
    {/if}
  </div>
</header>

<style>
  .status-header {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-4);
    min-height: 4.25rem;
    padding: var(--space-3) var(--space-5);
    border-bottom: 1px solid var(--border);
    background: var(--surface-raised);
  }

  .brand,
  .device-status,
  .device-info,
  .save-area {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .brand {
    font-weight: 700;
    letter-spacing: 0.03em;
  }

  .brand-mark {
    display: grid;
    width: 1.8rem;
    height: 1.8rem;
    place-items: center;
    border-radius: var(--radius-sm);
    background: var(--accent-strong);
    color: var(--surface-base);
  }

  .device-status {
    flex-wrap: wrap;
  }

  .status-pill {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-muted);
    font-size: 0.8rem;
  }

  .status-pill.connected {
    border-color: color-mix(in srgb, var(--success) 45%, var(--border));
    color: var(--success);
  }

  .device-info {
    flex-wrap: wrap;
  }

  .muted,
  .save-state {
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .save-area {
    margin-left: auto;
    flex-wrap: wrap;
  }

  .save-state.dirty {
    color: var(--warning);
  }

  .save-area button {
    min-height: 2.25rem;
    padding: 0 var(--space-3);
    border-color: var(--accent-strong);
    background: var(--accent-strong);
    color: #fff;
  }

  .save-feedback {
    color: var(--success);
    font-size: 0.8rem;
  }

  @media (max-width: 760px) {
    .status-header {
      gap: var(--space-2) var(--space-3);
      padding: var(--space-3);
    }

    .save-area {
      width: 100%;
      margin-left: 0;
    }
  }
</style>
