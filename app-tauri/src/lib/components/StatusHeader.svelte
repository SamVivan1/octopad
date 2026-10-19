<script lang="ts">
  /**
   * Compact chassis header.
   *
   * Everything here is read straight from the shared snapshot store — the
   * header never asks the core directly, and it never patches state itself, so
   * what it shows is always exactly what the core last reported.
   */
  import { appState } from '../state';

  const snapshot = $derived($appState);

  const configStateLabel = $derived(snapshot?.dirty ? 'Unsaved changes' : 'Saved');
  const configStateTone = $derived(snapshot?.dirty ? 'warn' : 'ok');

  const serialLabel = $derived(
    snapshot === null
      ? 'Serial: connecting'
      : snapshot.serial.connected
        ? snapshot.serial.port
          ? `Serial: Connected (${snapshot.serial.port})`
          : 'Serial: Connected'
        : 'Serial: Disconnected'
  );
  const serialTone = $derived(snapshot?.serial.connected ? 'ok' : '');

  const bleLabel = $derived(
    snapshot === null
      ? 'Bluetooth: connecting'
      : snapshot.ble.connected
        ? 'Bluetooth: Connected'
        : 'Bluetooth: Disconnected'
  );
  const bleTone = $derived(snapshot?.ble.connected ? 'ok' : '');

  const deviceLabel = $derived(`Device: ${snapshot?.device.name ?? 'Unnamed'}`);
  const firmwareLabel = $derived(`Firmware: ${snapshot?.device.firmwareVersion ?? 'unknown'}`);
</script>

<header class="status-header">
  <h1 class="status-header__brand">
    <span class="status-header__mark" aria-hidden="true"></span>
    OctoPad
  </h1>

  <div class="status-header__status">
    <span class="pill pill--{configStateTone}" data-testid="status-config-state">
      {configStateLabel}
    </span>

    <span
      class="pill pill--{serialTone}"
      data-testid="status-serial"
      title={snapshot?.serial.port ?? undefined}
    >
      {serialLabel}
    </span>

    <span class="pill pill--{bleTone}" data-testid="status-ble" title={snapshot?.ble.message}>
      {bleLabel}
    </span>

    <span class="status-header__meta" data-testid="status-device">{deviceLabel}</span>
    <span class="status-header__meta" data-testid="status-firmware">{firmwareLabel}</span>
  </div>
</header>

<style>
  .status-header {
    display: flex;
    align-items: center;
    gap: var(--space-5);
    min-height: var(--header-height);
    padding: var(--space-3) var(--space-5);
    border-bottom: 1px solid var(--border-subtle);
    background-color: var(--surface-base);
  }

  .status-header__brand {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin: 0;
    font-size: var(--font-size-lg);
    font-weight: 800;
    letter-spacing: 0.02em;
    white-space: nowrap;
  }

  /* A single keycap, echoing the device this app configures. */
  .status-header__mark {
    width: 18px;
    height: 18px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--accent-soft);
    box-shadow: inset 0 -2px 0 var(--accent);
  }

  .status-header__status {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .status-header__meta {
    font-size: var(--font-size-sm);
    color: var(--text-muted);
    white-space: nowrap;
  }

  @media (max-width: 640px) {
    .status-header {
      align-items: flex-start;
      flex-direction: column;
      gap: var(--space-3);
    }
  }
</style>