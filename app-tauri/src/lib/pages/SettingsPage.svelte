<script module lang="ts">
  /**
   * `DeviceManager::rename_device` trims the name and refuses an empty one or
   * anything longer than this, so the limit is pinned here and checked before
   * a command is issued rather than discovered as a failed write.
   */
  export const DEVICE_NAME_MAX = 24;
</script>

<script lang="ts">
  /**
   * Settings — the three firmware timings, the device name, what the pad
   * currently reports about itself, and the login/startup behaviour.
   *
   * The same three rules as the rest of the app apply:
   *
   * - **Nothing is edited locally.** The timings come from the snapshot, the
   *   name comes from `device-name-changed`, and the status block only reports
   *   what the core last told us. A refused command therefore leaves the page
   *   showing the truth.
   * - **Failures are visible.** Every device command runs through `runCommand`,
   *   which records a refusal or an IPC error in `lastError`; this page shows
   *   that message next to the control it came from.
   * - **The pad is only told what the pad accepted.** A rename is sent through
   *   the transport, and the input goes back to the reported name when the
   *   write fails.
   *
   * Autostart follows the same rule from the other side: the flag lives in the
   * OS, so the page shows only what `tauri-plugin-autostart` reports and
   * re-reads after every write. A failed read leaves the toggle disabled
   * ("unavailable") rather than showing a made-up position.
   */
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';

  import TimingControls from '../components/TimingControls.svelte';
  import * as api from '../api';
  import { autostartEnabled, readAutostart, setAutostart } from '../autostart';
  import { appState, lastError, runCommand } from '../state';

  const snapshot = $derived($appState);

  // -- device status ---------------------------------------------------------
  //
  // Read-only report of the pad: which transport is live and what it said.
  // The transport is derived rather than stored so it can never disagree with
  // the connection flags.

  const transport = $derived(
    snapshot === null
      ? 'Unknown'
      : snapshot.serial.connected
        ? 'Serial'
        : snapshot.ble.connected
          ? 'Bluetooth'
          : 'None'
  );

  const connectionMessage = $derived(
    snapshot === null
      ? 'Connecting'
      : snapshot.serial.connected
        ? (snapshot.serial.port ?? 'Connected')
        : snapshot.ble.connected
          ? (snapshot.ble.message || 'Connected')
          : 'Not connected'
  );

  // -- device rename ---------------------------------------------------------

  /** The name the core last reported; the input's source of truth. */
  function storedName(): string {
    return snapshot?.device.name ?? '';
  }

  let nameDraft = $state('');
  let nameInput = $state<HTMLInputElement | undefined>(undefined);
  let renameError = $state<string | null>(null);
  let renameResult = $state<string | null>(null);
  let renaming = $state(false);

  // Follow the core's name, and only then: a draft the user is typing is never
  // overwritten by a snapshot that still holds the previous name.
  let reported: string | null = null;
  $effect(() => {
    const name = storedName();
    if (reported === null || reported !== name) {
      nameDraft = name;
      reported = name;
    }
  });

  /**
   * Put the name the core reports back into the field.
   *
   * The element is written as well as the draft: a rejected edit can leave the
   * draft byte-identical to what is already in the field, and the binding alone
   * would then keep the refused text on screen.
   */
  function restoreReportedName(): void {
    nameDraft = storedName();
    if (nameInput) nameInput.value = nameDraft;
  }

  /** Refuse a name before it reaches the device, and restore the reported one. */
  function refuseRename(message: string): void {
    renameError = message;
    renameResult = null;
    restoreReportedName();
  }

  async function rename(): Promise<void> {
    // Read the field itself rather than the draft: this is the text the user
    // typed, whichever sequence of input/change events produced it.
    const name = (nameInput?.value ?? nameDraft).trim();
    nameDraft = name;
    renameError = null;
    renameResult = null;

    if (name.length === 0) {
      refuseRename('Enter a device name.');
      return;
    }
    if (name.length > DEVICE_NAME_MAX) {
      refuseRename(`The device name must be at most ${DEVICE_NAME_MAX} characters.`);
      return;
    }

    renaming = true;
    const renamed = await runCommand('rename device', () => api.renameDevice(name));
    renaming = false;

    if (renamed) {
      renameResult = `Renamed the pad to ${name}.`;
      return;
    }
    // The transport write failed, so the pad still carries its old name.
    renameError = get(lastError) ?? 'The device could not be renamed.';
    restoreReportedName();
  }

  // -- autostart --------------------------------------------------------------
  //
  // The login entry is OS state, not app state, so the toggle is a mirror: it is
  // filled in by reading the platform, and every write is confirmed by reading
  // it again. `autostartEnabled === null` means "not known" — a failed read
  // disables the control instead of guessing a position.

  let autostartLoaded = $state(false);
  let autostartBusy = $state(false);
  let autostartError = $state<string | null>(null);
  let autostartInput = $state<HTMLInputElement | undefined>(undefined);

  const autostartOn = $derived($autostartEnabled === true);
  const autostartUnavailable = $derived(autostartLoaded && $autostartEnabled === null);
  const autostartStatus = $derived(
    !autostartLoaded
      ? 'Reading the login entry…'
      : autostartUnavailable
        ? 'The autostart state is unavailable.'
        : autostartOn
          ? 'OctoPad starts at login.'
          : 'OctoPad does not start at login.'
  );

  onMount(() => {
    void readAutostart().finally(() => {
      autostartLoaded = true;
    });
  });

  /**
   * Put the platform state back on the control.
   *
   * Written as well as derived: a write that fails leaves the store's value
   * unchanged, so nothing re-renders and the click's own `checked` flip would
   * otherwise stay on screen as a change that never happened.
   */
  function syncAutostartControl(): void {
    if (autostartInput) {
      autostartInput.checked = autostartOn;
    }
  }

  async function toggleAutostart(): Promise<void> {
    const requested = autostartInput?.checked ?? !autostartOn;
    autostartBusy = true;
    autostartError = null;

    const changed = await setAutostart(requested);
    autostartBusy = false;

    if (!changed) {
      autostartError = get(lastError) ?? 'The autostart setting could not be changed.';
    }
    syncAutostartControl();
  }
</script>

<section class="page" data-page="settings" data-testid="active-page">
  <h1 class="page__title">Settings</h1>

  <div class="card">
    <h2 class="section-title">Timings</h2>
    <TimingControls />
    <p class="muted settings__hint">
      Firmware defaults are 25 ms debounce, 250 ms click timeout and 450 ms hold timeout. Changes
      are written to the pad the next time the configuration is saved.
    </p>
  </div>

  <div class="card">
    <h2 class="section-title">Device</h2>

    <form
      class="settings__rename"
      data-testid="settings-rename"
      onsubmit={(event) => {
        event.preventDefault();
        void rename();
      }}
    >
      <div class="settings__field">
        <label class="settings__label" for="deviceName">Device name</label>
        <input
          id="deviceName"
          class="settings__input"
          type="text"
          autocomplete="off"
          spellcheck="false"
          maxlength={DEVICE_NAME_MAX}
          data-testid="device-name-input"
          aria-invalid={renameError ? 'true' : undefined}
          aria-describedby={renameError ? 'device-name-error' : undefined}
          bind:this={nameInput}
          bind:value={nameDraft}
        />
      </div>

      <button type="submit" class="btn btn--primary" disabled={renaming} data-testid="device-rename">
        Rename device
      </button>
    </form>

    {#if renameError}
      <p
        class="alert alert--error settings__alert"
        id="device-name-error"
        role="alert"
        data-testid="settings-error"
      >
        {renameError}
      </p>
    {/if}

    {#if renameResult}
      <p class="alert alert--ok settings__alert" role="status" data-testid="settings-success">
        {renameResult}
      </p>
    {/if}

    <p class="muted settings__hint">
      Renaming writes a NAME command over the active connection, so it needs the pad to be
      connected. Nothing is saved to the configuration file.
    </p>

    <dl class="settings__facts" data-testid="settings-status">
      <dt>Device name</dt>
      <dd data-testid="settings-device-name">{snapshot?.device.name ?? 'Unnamed'}</dd>

      <dt>Firmware</dt>
      <dd class="mono" data-testid="settings-firmware">
        {snapshot?.device.firmwareVersion ?? 'unknown'}
      </dd>

      <dt>Transport</dt>
      <dd data-testid="settings-transport">{transport}</dd>

      <dt>Connection</dt>
      <dd data-testid="settings-connection-message">{connectionMessage}</dd>
    </dl>
  </div>

  <div class="card">
    <h2 class="section-title">Application</h2>

    <div class="settings__toggle">
      <input
        id="autostart"
        class="settings__checkbox"
        type="checkbox"
        aria-describedby="autostart-status"
        data-testid="autostart-toggle"
        checked={autostartOn}
        disabled={autostartBusy || autostartUnavailable}
        onchange={toggleAutostart}
        bind:this={autostartInput}
      />
      <label class="settings__label" for="autostart">Start OctoPad at login</label>
    </div>

    <p class="muted settings__hint" id="autostart-status" data-testid="autostart-status">
      {autostartStatus}
    </p>

    {#if autostartError}
      <p class="alert alert--error settings__alert" role="alert" data-testid="autostart-error">
        {autostartError}
      </p>
    {/if}

    <p class="muted settings__hint">
      Started at login, OctoPad opens in the tray with its window hidden. Closing the window hides
      it as well and keeps the device connections running; use Quit in the tray menu to exit.
    </p>
  </div>
</section>

<style>
  .settings__rename {
    display: flex;
    align-items: flex-end;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .settings__field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    flex: 1 1 200px;
    min-width: 0;
  }

  .settings__label {
    font-size: var(--font-size-sm);
    color: var(--text-muted);
  }

  .settings__input {
    min-height: 30px;
    padding: 4px 8px;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background-color: var(--surface-base);
    color: var(--text-primary);
    font-family: inherit;
    font-size: var(--font-size-sm);
  }

  .settings__input[aria-invalid='true'] {
    border-color: var(--error);
  }

  .settings__toggle {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .settings__checkbox {
    width: 16px;
    height: 16px;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .settings__checkbox:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .settings__alert {
    margin: var(--space-3) 0 0;
  }

  .settings__hint {
    margin: var(--space-4) 0 0;
    font-size: var(--font-size-sm);
  }

  .settings__facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-2) var(--space-5);
    margin: var(--space-5) 0 0;
    padding-top: var(--space-4);
    border-top: 1px solid var(--border-subtle);
    font-size: var(--font-size-md);
  }

  .settings__facts dt {
    color: var(--text-muted);
    font-size: var(--font-size-sm);
  }

  .settings__facts dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  @media (max-width: 640px) {
    .settings__rename {
      align-items: stretch;
      flex-direction: column;
    }
  }
</style>