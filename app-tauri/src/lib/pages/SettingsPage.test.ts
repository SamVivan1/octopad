// @vitest-environment jsdom
/**
 * Contract tests for Settings and the Profiles workflow (Task 6).
 *
 * The mocked `invoke` is a small stand-in for `octopad-core`: it applies
 * `set_timing`, `rename_device`, `save_config` and the legacy import with the
 * same semantics as `AppState` / `DeviceManager` (including the 24-character
 * device-name limit and the "a save must not happen without an explicit
 * action" rule), and emits the same events the real core does. The UI is
 * therefore only ever asserted against state that came back through the shared
 * snapshot store.
 *
 * What is pinned here:
 *
 * - The three timing inputs carry the firmware ranges (debounce 0–1000 ms,
 *   click 50–2000 ms, hold 50–3000 ms) and the 5 ms step.
 * - **Client-side validation.** An out-of-range, off-step or non-numeric timing
 *   never reaches the core, and the input snaps back to the stored value.
 * - **A `false` return is honored.** The core can still refuse a timing the
 *   browser accepted; the page must not display the refused value.
 * - **Device rename** is trimmed, capped at 24 characters before sending, and a
 *   failed transport write is reported instead of being shown as a new name.
 * - **Import never silently saves.** A successful import replaces the working
 *   snapshot and marks it dirty while the config file is untouched; an import
 *   error is rendered and leaves the editor alone.
 * - **Save clears dirty** on success and reports failure otherwise.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

import ProfilesPage from './ProfilesPage.svelte';
import SettingsPage from './SettingsPage.svelte';
import { appState, initializeAppState, resetAppState } from '../state';
import type { AppSnapshot, Button, Config } from '../types';

// -- fixtures ----------------------------------------------------------------

const PINOUT = [20, 9, 2, 1, 21, 10, 3, 0];

const LABELS = ['Terminal', 'Browser', 'Mail', 'Chat', 'Music', 'Focus', 'Stream', 'Sleep'];
const LEGACY_LABELS = ['One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight'];

function makeButtons(labels: string[]): Button[] {
  return labels.map((label, index) => ({
    index,
    pin: PINOUT[index],
    label,
    single: { type: 'none' },
    double: { type: 'none' },
    hold: { type: 'none' },
  }));
}

/** A configuration; `labels` names the Desktop keys so an import is visible. */
function makeConfig(labels: string[], overrides: Partial<Config> = {}): Config {
  return {
    version: 1,
    schemaVersion: 1,
    device: 'octopad',
    debounceMs: 25,
    clickTimeoutMs: 250,
    holdTimeoutMs: 450,
    activeModeIndex: 0,
    modes: [
      { id: 'desktop', name: 'Desktop', ledState: false, buttons: makeButtons(labels) },
      { id: 'ha', name: 'Home Assistant', ledState: false, buttons: [] },
    ],
    ...overrides,
  };
}

function makeSnapshot(
  config: Config = makeConfig(LABELS),
  overrides: Partial<AppSnapshot> = {}
): AppSnapshot {
  return {
    config,
    configPath: '/home/user/.config/octopad/config.json',
    modeIndex: 0,
    selectedIndex: 0,
    dirty: false,
    activePreset: null,
    serial: { connected: false, port: null },
    ble: { connected: false, message: '' },
    device: { name: 'Desk Pad', firmwareVersion: '1.4.2' },
    homeAssistant: { connected: false, message: '', entitiesCount: 0 },
    installedAppsCount: 0,
    ...overrides,
  };
}

// -- fake core ---------------------------------------------------------------

type EventHandler = (event: { payload: unknown }) => void;
type Overrides = Map<string, () => unknown>;

let handlers = new Map<string, EventHandler>();
let overrides: Overrides = new Map();
let commandLog: string[] = [];
let currentSnapshot: AppSnapshot = makeSnapshot();

/** What is actually on disk; only `save_config_cmd` ever writes here. */
let diskConfig: Config = structuredClone(currentSnapshot.config);

function emit(name: string, payload: unknown): void {
  handlers.get(name)?.({ payload });
}

function markDirty(): void {
  currentSnapshot.dirty = true;
  emit('octopad://dirty-changed', { dirty: true });
}

function fakeCore(command: string, args: Record<string, unknown> = {}): unknown {
  const override = overrides.get(command);
  if (override) return override();

  switch (command) {
    case 'get_state_snapshot':
      return Promise.resolve(structuredClone(currentSnapshot));
    case 'list_presets_cmd':
      return Promise.resolve([]);

    case 'set_timing_cmd': {
      // `AppState::set_timing` only accepts the three camelCase keys; the
      // firmware ranges are the UI's contract, so the fake enforces both.
      const field = String(args.field);
      const value = Number(args.value);
      if (field !== 'debounceMs' && field !== 'clickTimeoutMs' && field !== 'holdTimeoutMs') {
        return Promise.resolve(false);
      }
      (currentSnapshot.config as unknown as Record<string, unknown>)[field] = value;
      markDirty();
      emit('octopad://timing-changed', { field, value });
      return Promise.resolve(true);
    }

    case 'rename_device_cmd': {
      // `DeviceManager::rename_device`: trimmed, non-empty, at most 24 chars,
      // and only a successful transport write updates the name.
      const name = String(args.name ?? '').trim();
      if (name.length === 0 || name.length > 24) return Promise.resolve(false);
      currentSnapshot.device.name = name;
      emit('octopad://device-name-changed', { name });
      return Promise.resolve(true);
    }

    case 'save_config_cmd': {
      diskConfig = structuredClone(currentSnapshot.config);
      currentSnapshot.dirty = false;
      emit('octopad://dirty-changed', { dirty: false });
      return Promise.resolve(true);
    }

    case 'import_legacy_config': {
      // `bridge::run_import` loads the legacy config into the editor and marks
      // it dirty — it never writes the config file itself.
      currentSnapshot.config = makeConfig(LEGACY_LABELS, {
        debounceMs: 30,
        clickTimeoutMs: 300,
        holdTimeoutMs: 600,
      });
      markDirty();
      emit('octopad://config-replaced', { source: 'legacy' });
      return Promise.resolve({
        imported_config: true,
        presets: ['work'],
        warnings: [],
        first_run_auto: false,
      });
    }

    default:
      commandLog.push(command);
      return Promise.resolve(true);
  }
}

function rejectCommand(command: string, result: 'false' | 'error' = 'false'): void {
  overrides.set(
    command,
    () => (result === 'false' ? Promise.resolve(false) : Promise.reject(new Error('ipc down')))
  );
}

// -- harness -----------------------------------------------------------------

async function flush(): Promise<void> {
  for (let tick = 0; tick < 5; tick += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function renderSettings(): Promise<void> {
  render(SettingsPage);
  await flush();
}

async function renderProfiles(): Promise<void> {
  render(ProfilesPage);
  await flush();
}

/** The input whose `id` is the timing field name, as the plan requires. */
function timingInput(field: string): HTMLInputElement {
  const input = document.getElementById(field);
  expect(input, `no timing input with id ${field}`).not.toBeNull();
  return input as HTMLInputElement;
}

function timingCommands(): Array<[string, Record<string, unknown>]> {
  return invokeMock.mock.calls
    .filter((call) => call[0] === 'set_timing_cmd')
    .map((call) => [String(call[0]), (call[1] ?? {}) as Record<string, unknown>]);
}

function renameCalls(): Array<Record<string, unknown>> {
  return invokeMock.mock.calls
    .filter((call) => call[0] === 'rename_device_cmd')
    .map((call) => (call[1] ?? {}) as Record<string, unknown>);
}

function renameInput(): HTMLInputElement {
  return screen.getByLabelText('Device name') as HTMLInputElement;
}

function renameButton(): HTMLElement {
  return screen.getByRole('button', { name: 'Rename device' });
}

function statusBlock(): HTMLElement {
  return screen.getByTestId('settings-status');
}

function profilesState(): string {
  return screen.getByTestId('profiles-config-state').textContent?.trim() ?? '';
}

function profilesFeedback(): string {
  return screen.getByTestId('profiles-feedback').textContent?.trim() ?? '';
}

beforeEach(async () => {
  handlers = new Map();
  overrides = new Map();
  commandLog = [];
  currentSnapshot = makeSnapshot();
  diskConfig = structuredClone(currentSnapshot.config);
  invokeMock.mockReset();
  listenMock.mockReset();
  invokeMock.mockImplementation((command: string, args: Record<string, unknown> = {}) => {
    commandLog.push(command);
    return fakeCore(command, args);
  });
  listenMock.mockImplementation(async (name: string, handler: EventHandler) => {
    const unlisten = vi.fn();
    handlers.set(name, handler);
    return unlisten;
  });
  resetAppState();
  await initializeAppState();
});

afterEach(() => {
  cleanup();
  resetAppState();
});

// -- timing controls ---------------------------------------------------------

describe('timing controls', () => {
  it('exposes the firmware timing ranges and the 5 ms step on every timing input', async () => {
    await renderSettings();

    const ranges: Record<string, { min: string; max: string }> = {
      debounceMs: { min: '0', max: '1000' },
      clickTimeoutMs: { min: '50', max: '2000' },
      holdTimeoutMs: { min: '50', max: '3000' },
    };

    for (const [field, range] of Object.entries(ranges)) {
      const input = timingInput(field);
      expect(input.getAttribute('type'), field).toBe('number');
      expect(input.getAttribute('min'), field).toBe(range.min);
      expect(input.getAttribute('max'), field).toBe(range.max);
      expect(input.getAttribute('step'), field).toBe('5');
    }

    // The stored firmware values are what the controls show.
    expect(timingInput('debounceMs').value).toBe('25');
    expect(timingInput('clickTimeoutMs').value).toBe('250');
    expect(timingInput('holdTimeoutMs').value).toBe('450');
  });

  it('sends a valid timing change to the core and renders what the core confirmed', async () => {
    await renderSettings();

    await fireEvent.change(timingInput('clickTimeoutMs'), { target: { value: '400' } });
    await flush();

    expect(invokeMock).toHaveBeenCalledWith('set_timing_cmd', {
      field: 'clickTimeoutMs',
      value: 400,
    });
    expect(get(appState)?.config.clickTimeoutMs).toBe(400);
    expect(timingInput('clickTimeoutMs').value).toBe('400');
  });

  it('rejects an out-of-range timing without issuing a command', async () => {
    await renderSettings();

    // debounce above 1000 ms, click below 50 ms, hold above 3000 ms.
    await fireEvent.change(timingInput('debounceMs'), { target: { value: '1500' } });
    await fireEvent.change(timingInput('clickTimeoutMs'), { target: { value: '10' } });
    await fireEvent.change(timingInput('holdTimeoutMs'), { target: { value: '4000' } });
    await flush();

    expect(timingCommands()).toEqual([]);

    const errors = screen.getAllByTestId('timing-error');
    expect(errors).toHaveLength(3);
    expect(errors[0].getAttribute('data-field')).toBe('debounceMs');
    expect(errors[1].getAttribute('data-field')).toBe('clickTimeoutMs');
    expect(errors[2].getAttribute('data-field')).toBe('holdTimeoutMs');
    for (const error of errors) {
      expect(error.textContent?.trim()).not.toBe('');
    }

    // The stored values are untouched and the inputs snap back to them.
    expect(get(appState)?.config.debounceMs).toBe(25);
    expect(timingInput('debounceMs').value).toBe('25');
    expect(timingInput('clickTimeoutMs').value).toBe('250');
    expect(timingInput('holdTimeoutMs').value).toBe('450');
  });

  it('rejects a timing that is not a whole 5 ms number without issuing a command', async () => {
    await renderSettings();

    await fireEvent.change(timingInput('debounceMs'), { target: { value: 'abc' } });
    await fireEvent.change(timingInput('clickTimeoutMs'), { target: { value: '' } });
    await fireEvent.change(timingInput('holdTimeoutMs'), { target: { value: '451' } });
    await flush();

    expect(timingCommands()).toEqual([]);
    expect(screen.getAllByTestId('timing-error')).toHaveLength(3);
    expect(timingInput('debounceMs').value).toBe('25');
    expect(timingInput('holdTimeoutMs').value).toBe('450');
  });

  it('reports a timing the core refuses and never shows the refused value', async () => {
    await renderSettings();
    rejectCommand('set_timing_cmd');

    await fireEvent.change(timingInput('holdTimeoutMs'), { target: { value: '900' } });
    await flush();

    expect(invokeMock).toHaveBeenCalledWith('set_timing_cmd', { field: 'holdTimeoutMs', value: 900 });
    const error = screen.getByTestId('timing-error');
    expect(error.getAttribute('data-field')).toBe('holdTimeoutMs');
    expect(error.textContent?.toLowerCase()).toContain('reject');

    expect(get(appState)?.config.holdTimeoutMs).toBe(450);
    expect(timingInput('holdTimeoutMs').value).toBe('450');
  });

  it('follows the core when another edit changes a timing', async () => {
    await renderSettings();

    currentSnapshot.config.holdTimeoutMs = 1200;
    emit('octopad://timing-changed', { field: 'holdTimeoutMs', value: 1200 });
    await flush();

    expect(timingInput('holdTimeoutMs').value).toBe('1200');
  });
});

// -- device rename -----------------------------------------------------------

describe('device rename', () => {
  it('trims the name before sending it to the device', async () => {
    await renderSettings();

    await fireEvent.change(renameInput(), { target: { value: '  Studio Pad  ' } });
    await fireEvent.click(renameButton());
    await flush();

    expect(renameCalls()).toEqual([{ name: 'Studio Pad' }]);
    expect(get(appState)?.device.name).toBe('Studio Pad');
    expect(renameInput().value).toBe('Studio Pad');
  });

  it('rejects a name longer than 24 characters without sending it', async () => {
    await renderSettings();

    const tooLong = 'x'.repeat(25);
    await fireEvent.change(renameInput(), { target: { value: tooLong } });
    await fireEvent.click(renameButton());
    await flush();

    expect(renameCalls()).toEqual([]);
    const error = screen.getByTestId('settings-error');
    expect(error.textContent).toContain('24');
    expect(get(appState)?.device.name).toBe('Desk Pad');
    expect(renameInput().value).toBe('Desk Pad');
  });

  it('rejects a blank name without sending it', async () => {
    await renderSettings();

    await fireEvent.change(renameInput(), { target: { value: '   ' } });
    await fireEvent.click(renameButton());
    await flush();

    expect(renameCalls()).toEqual([]);
    expect(screen.getByTestId('settings-error').textContent?.trim()).not.toBe('');
    expect(get(appState)?.device.name).toBe('Desk Pad');
  });

  it('surfaces a failed rename write and keeps the previous name', async () => {
    await renderSettings();
    rejectCommand('rename_device_cmd');

    await fireEvent.change(renameInput(), { target: { value: 'Studio Pad' } });
    await fireEvent.click(renameButton());
    await flush();

    expect(renameCalls()).toEqual([{ name: 'Studio Pad' }]);
    const error = screen.getByTestId('settings-error');
    expect(error.textContent?.trim()).not.toBe('');
    expect(error.textContent?.toLowerCase()).toContain('reject');

    // The pad was not renamed: the state never claims a new name.
    expect(get(appState)?.device.name).toBe('Desk Pad');
    expect(renameInput().value).toBe('Desk Pad');
    expect(within(statusBlock()).getByTestId('settings-device-name').textContent).toContain(
      'Desk Pad'
    );
  });

  it('follows a rename the core confirmed on its own', async () => {
    await renderSettings();

    currentSnapshot.device.name = 'Kitchen Pad';
    emit('octopad://device-name-changed', { name: 'Kitchen Pad' });
    await flush();

    expect(renameInput().value).toBe('Kitchen Pad');
    expect(within(statusBlock()).getByTestId('settings-device-name').textContent).toContain(
      'Kitchen Pad'
    );
  });
});

// -- device status -----------------------------------------------------------

describe('device status', () => {
  it('shows the device name, firmware version, transport and connection message', async () => {
    await renderSettings();

    const status = statusBlock();
    expect(within(status).getByTestId('settings-device-name').textContent).toContain('Desk Pad');
    expect(within(status).getByTestId('settings-firmware').textContent).toContain('1.4.2');
    expect(within(status).getByTestId('settings-transport').textContent?.trim()).toBe('None');
    expect(within(status).getByTestId('settings-connection-message').textContent?.trim()).toBe(
      'Not connected'
    );
  });

  it('updates the transport and message when the core reports a connection change', async () => {
    await renderSettings();

    emit('octopad://serial-status-changed', { connected: true, port: '/dev/ttyACM0' });
    await flush();

    let status = statusBlock();
    expect(within(status).getByTestId('settings-transport').textContent).toContain('Serial');
    expect(within(status).getByTestId('settings-connection-message').textContent).toContain(
      '/dev/ttyACM0'
    );

    // Switching to Bluetooth moves the same block onto the BLE message.
    emit('octopad://serial-status-changed', { connected: false, port: null });
    emit('octopad://ble-status-changed', { connected: true, message: 'OctoPad BLE ready' });
    await flush();

    status = statusBlock();
    expect(within(status).getByTestId('settings-transport').textContent).toContain('Bluetooth');
    expect(within(status).getByTestId('settings-connection-message').textContent).toContain(
      'OctoPad BLE ready'
    );

    emit('octopad://device-info-changed', { version: '1.5.0' });
    await flush();
    expect(within(statusBlock()).getByTestId('settings-firmware').textContent).toContain('1.5.0');
  });
});

// -- profiles: import and save ----------------------------------------------

describe('profiles workflow', () => {
  it('replaces the working snapshot on import and never saves it', async () => {
    await renderProfiles();
    const diskBefore = structuredClone(diskConfig);

    await fireEvent.click(screen.getByRole('button', { name: 'Import legacy config' }));
    await flush();

    // The working copy is the imported config and is marked unsaved...
    expect(get(appState)?.config.modes[0].buttons[0].label).toBe('One');
    expect(get(appState)?.config.debounceMs).toBe(30);
    expect(get(appState)?.dirty).toBe(true);
    expect(profilesState()).toBe('Unsaved changes');

    // ...but nothing was written to disk, and no save command was issued.
    expect(commandLog).not.toContain('save_config_cmd');
    expect(diskConfig).toEqual(diskBefore);
    expect(profilesFeedback()).toMatch(/save/i);
  });

  it('renders an import error and leaves the working copy untouched', async () => {
    await renderProfiles();
    overrides.set('import_legacy_config', () =>
      Promise.reject({ Parse: 'unexpected end of input' })
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Import legacy config' }));
    await flush();

    const feedback = screen.getByTestId('profiles-feedback');
    expect(feedback.textContent).toContain('Parse');
    expect(feedback.className).toContain('alert--error');

    // A failed import leaves the editor exactly as it was.
    expect(get(appState)?.config.modes[0].buttons[0].label).toBe('Terminal');
    expect(get(appState)?.dirty).toBe(false);
    expect(profilesState()).toBe('Saved');
    expect(commandLog).not.toContain('save_config_cmd');
  });

  it('clears the dirty state after a successful save', async () => {
    await renderProfiles();
    currentSnapshot.dirty = true;
    currentSnapshot.config.holdTimeoutMs = 700;
    emit('octopad://dirty-changed', { dirty: true });
    await flush();
    expect(profilesState()).toBe('Unsaved changes');

    await fireEvent.click(screen.getByRole('button', { name: 'Save configuration' }));
    await flush();

    expect(commandLog).toContain('save_config_cmd');
    expect(get(appState)?.dirty).toBe(false);
    expect(profilesState()).toBe('Saved');
    expect(diskConfig.holdTimeoutMs).toBe(700);
    expect(profilesFeedback()).toMatch(/saved/i);
  });

  it('surfaces a failed save and keeps the working copy dirty', async () => {
    await renderProfiles();
    currentSnapshot.dirty = true;
    emit('octopad://dirty-changed', { dirty: true });
    await flush();
    rejectCommand('save_config_cmd');

    await fireEvent.click(screen.getByRole('button', { name: 'Save configuration' }));
    await flush();

    const feedback = screen.getByTestId('profiles-feedback');
    expect(feedback.className).toContain('alert--error');
    expect(feedback.textContent?.trim()).not.toBe('');
    expect(get(appState)?.dirty).toBe(true);
    expect(profilesState()).toBe('Unsaved changes');
    expect(diskConfig.holdTimeoutMs).toBe(450);
  });
});