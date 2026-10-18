<script lang="ts">
  import { appState, refreshAppSnapshot } from '../state';
  import { renameDevice, setTiming } from '../api';
  import TimingControls from '../components/TimingControls.svelte';
  import type { AppSnapshot, TimingField } from '../types';

  let busy = false;
  let feedback = '';
  let renameDraft = '';
  let renameEdited = false;

  $: snapshot = $appState.snapshot;
  $: if (snapshot && !busy && !renameEdited) {
    renameDraft = snapshot.device.name ?? '';
  }
  $: transport = getTransport(snapshot);
  $: connectionMessage = getConnectionMessage(snapshot);

  function getTransport(current: AppSnapshot | null): string {
    if (current?.serial.connected) return 'Serial';
    if (current?.ble.connected) return 'Bluetooth LE';
    return current ? 'Disconnected' : 'Loading…';
  }

  function getConnectionMessage(current: AppSnapshot | null): string {
    if (!current) return 'Loading device status…';
    if (current.serial.connected) return current.serial.port ?? 'Serial device connected';
    if (current.ble.connected) return current.ble.message || 'Connected over Bluetooth LE';
    return current.ble.message || 'No device connected';
  }

  async function updateTiming(field: TimingField, value: number) {
    if (busy) return;
    busy = true;
    feedback = '';
    try {
      if (!(await setTiming(field, value))) {
        feedback = 'Timing update failed';
        return;
      }
      await refreshAppSnapshot();
    } catch (error) {
      feedback = `Timing update failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      busy = false;
    }
  }

  async function rename() {
    if (busy) return;
    const name = renameDraft.trim();
    const length = Array.from(name).length;
    if (length < 1 || length > 24) {
      feedback = 'Device name must contain 1–24 characters.';
      return;
    }

    busy = true;
    feedback = '';
    try {
      if (!(await renameDevice(name))) {
        feedback = 'Rename failed: the device did not accept the name.';
        return;
      }
      await refreshAppSnapshot();
      renameDraft = name;
      renameEdited = false;
      feedback = 'Device renamed';
    } catch (error) {
      feedback = `Rename failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      busy = false;
    }
  }
</script>

<section class="settings-page" aria-labelledby="settings-title">
  <h1 id="settings-title">Settings</h1>
  {#if feedback}
    <p class="feedback" role="alert">{feedback}</p>
  {/if}

  {#if snapshot}
    <div class="settings-grid">
      <section class="settings-card" aria-labelledby="device-title">
        <h2 id="device-title">Device</h2>
        <dl>
          <div>
            <dt>Device name</dt>
            <dd>{snapshot.device.name ?? 'Unknown'}</dd>
          </div>
          <div>
            <dt>Firmware version</dt>
            <dd>{snapshot.device.firmwareVersion ?? 'Unknown'}</dd>
          </div>
          <div>
            <dt>Current transport</dt>
            <dd>{transport}</dd>
          </div>
          <div>
            <dt>Connection message</dt>
            <dd>{connectionMessage}</dd>
          </div>
        </dl>

        <label for="device-name">Rename device</label>
        <div class="rename-row">
          <input
            id="device-name"
            aria-label="Device name"
            value={renameDraft}
            disabled={busy}
            oninput={(event) => {
              renameDraft = event.currentTarget.value;
              renameEdited = true;
            }}
          />
          <button type="button" disabled={busy} onclick={rename}>Rename device</button>
        </div>
      </section>

      <section class="settings-card" aria-label="Timing settings">
        <TimingControls
          values={snapshot.config}
          disabled={busy}
          onChange={updateTiming}
        />
      </section>
    </div>
  {:else}
    <p class="empty-state">Loading settings…</p>
  {/if}
</section>

<style>
  .settings-page h1 {
    margin: 0 0 var(--space-4);
    font-size: 1.5rem;
  }

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 22rem), 1fr));
    align-items: start;
    gap: var(--space-4);
  }

  .settings-card {
    display: grid;
    gap: var(--space-4);
    padding: var(--space-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-raised);
  }

  .settings-card h2 {
    margin: 0;
    font-size: 1.05rem;
  }

  dl {
    display: grid;
    gap: var(--space-3);
    margin: 0;
  }

  dl > div {
    display: grid;
    grid-template-columns: minmax(8rem, 0.75fr) minmax(0, 1fr);
    gap: var(--space-3);
  }

  dt {
    color: var(--text-muted);
  }

  dd {
    min-width: 0;
    margin: 0;
    overflow-wrap: anywhere;
  }

  .settings-card > label {
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .rename-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .rename-row input {
    flex: 1 1 10rem;
    min-width: 0;
    min-height: 2.5rem;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-base);
    color: var(--text);
  }

  .rename-row button {
    min-height: 2.5rem;
    padding: 0 var(--space-3);
  }

  .feedback {
    color: var(--error);
  }

  .empty-state {
    color: var(--text-muted);
  }
</style>
