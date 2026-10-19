/**
 * Contract tests for the typed frontend state bridge (`types.ts` / `api.ts` /
 * `state.ts`).
 *
 * Every `@tauri-apps` call is mocked, so the suite pins the two things the Rust
 * side cannot be asked about twice: the *order* of listener registration versus
 * the initial snapshot, and the exact wire shape the UI must tolerate.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

import {
  appState,
  clearLogs,
  initializeAppState,
  lastError,
  logEntries,
  presets,
  LOG_LIMIT,
  resetAppState,
  runCommand,
} from './state';
import {
  OCTOPAD_EVENTS,
  getStateSnapshot,
  listPresets,
  sendDeviceCommand,
  setMode,
  setTiming,
} from './api';
import type { AppSnapshot, Button } from './types';

// -- tauri mocks -------------------------------------------------------------

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

// -- fixtures ----------------------------------------------------------------

const PINOUT = [20, 9, 2, 1, 21, 10, 3, 0];

/** Every event the Rust core can emit, in the order the bridge forwards them. */
const EXPECTED_EVENT_NAMES: string[] = [
  'octopad://dirty-changed',
  'octopad://config-replaced',
  'octopad://button-changed',
  'octopad://timing-changed',
  'octopad://mode-changed',
  'octopad://selection-changed',
  'octopad://config-saved',
  'octopad://presets-changed',
  'octopad://serial-status-changed',
  'octopad://ble-status-changed',
  'octopad://device-name-changed',
  'octopad://device-info-changed',
  'octopad://serial-log',
  'octopad://ha-status-changed',
  'octopad://ha-entities-changed',
  'octopad://apps-changed',
];

function makeButton(index: number): Button {
  return {
    index,
    pin: PINOUT[index],
    label: `Button ${index + 1}`,
    // legacy single-dict trigger slots
    single: { type: 'none' },
    double: { type: 'none' },
    hold: { type: 'none' },
  };
}

function makeSnapshot(): AppSnapshot {
  return {
    config: {
      version: 1,
      schemaVersion: 1,
      device: 'octopad',
      debounceMs: 25,
      clickTimeoutMs: 250,
      holdTimeoutMs: 450,
      activeModeIndex: 0,
      modes: [
        {
          id: 'desktop',
          name: 'Desktop',
          ledState: false,
          buttons: [makeButton(0), makeButton(1), makeButton(2), makeButton(3)],
        },
        {
          id: 'ha',
          name: 'Home Assistant',
          ledState: false,
          buttons: [makeButton(0), makeButton(1), makeButton(2), makeButton(3)],
        },
      ],
    },
    configPath: '/home/user/.config/octopad/config.json',
    modeIndex: 0,
    selectedIndex: 0,
    dirty: false,
    activePreset: null,
    serial: { connected: false, port: null },
    ble: { connected: false, message: '' },
    device: { name: null, firmwareVersion: null },
    homeAssistant: { connected: false, message: '', entitiesCount: 0 },
    installedAppsCount: 0,
  };
}

// -- harness -----------------------------------------------------------------

type EventHandler = (event: { payload: unknown }) => void;

interface Registered {
  name: string;
  unlisten: ReturnType<typeof vi.fn>;
}

let handlers: Map<string, EventHandler> = new Map();
let registered: Registered[] = [];
let order: string[] = [];
let currentSnapshot: AppSnapshot = makeSnapshot();
let cleanups: Array<() => void> = [];

function defaultInvoke(command: string): unknown {
  order.push(`invoke:${command}`);
  if (command === 'get_state_snapshot') {
    return Promise.resolve(structuredClone(currentSnapshot));
  }
  if (command === 'list_presets_cmd') {
    return Promise.resolve(['desk-set', 'gaming']);
  }
  return Promise.resolve(true);
}

async function flush(): Promise<void> {
  for (let tick = 0; tick < 4; tick += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

function emit(eventName: string, payload: unknown): void {
  const handler = handlers.get(eventName);
  expect(handler, `no listener registered for ${eventName}`).toBeTypeOf('function');
  handler?.({ payload });
}

async function start(): Promise<() => void> {
  const cleanup = await initializeAppState();
  cleanups.push(cleanup);
  return cleanup;
}

beforeEach(() => {
  handlers = new Map();
  registered = [];
  order = [];
  currentSnapshot = makeSnapshot();
  cleanups = [];
  invokeMock.mockReset();
  listenMock.mockReset();
  invokeMock.mockImplementation((command: string) => defaultInvoke(command));
  listenMock.mockImplementation(async (name: string, handler: EventHandler) => {
    const unlisten = vi.fn();
    order.push(`listen:${name}`);
    registered.push({ name, unlisten });
    handlers.set(name, handler);
    return unlisten;
  });
  resetAppState();
});

afterEach(() => {
  for (const cleanup of cleanups) {
    cleanup();
  }
  cleanups = [];
  resetAppState();
});

// -- tests -------------------------------------------------------------------

describe('initializeAppState', () => {
  it('registers every listener before it requests or applies the snapshot', async () => {
    let resolveSnapshot: (snapshot: AppSnapshot) => void = () => {
      throw new Error('the snapshot was never requested');
    };
    invokeMock.mockImplementation((command: string) => {
      order.push(`invoke:${command}`);
      if (command === 'get_state_snapshot') {
        return new Promise<AppSnapshot>((resolve) => {
          resolveSnapshot = resolve;
        });
      }
      return Promise.resolve([]);
    });

    const initializing = initializeAppState();
    await flush();

    // the snapshot request is in flight …
    expect(order).toContain('invoke:get_state_snapshot');
    // … and nothing has been published yet.
    expect(get(appState)).toBeNull();

    // Explicit ordering assertion: the *first* invoke happens after the *last*
    // listener registration, so no event emitted during the snapshot fetch can
    // be missed.
    const firstInvoke = order.findIndex((entry) => entry.startsWith('invoke:'));
    const lastListen = order.map((entry) => entry.startsWith('listen:')).lastIndexOf(true);
    expect(firstInvoke).toBeGreaterThan(lastListen);

    // All sixteen core events are wired, with the exact wire names.
    expect(registered.map((entry) => entry.name)).toEqual(EXPECTED_EVENT_NAMES);

    resolveSnapshot(makeSnapshot());
    const cleanup = await initializing;
    cleanups.push(cleanup);

    expect(get(appState)).toEqual(makeSnapshot());
  });

  it('exposes the same event names through the typed api module', () => {
    expect(Object.values(OCTOPAD_EVENTS).slice().sort()).toEqual(
      EXPECTED_EVENT_NAMES.slice().sort()
    );
  });

  it('unregisters every listener on cleanup and stays idempotent', async () => {
    const cleanup = await start();

    expect(registered).toHaveLength(EXPECTED_EVENT_NAMES.length);
    expect(registered.every((entry) => entry.unlisten.mock.calls.length === 0)).toBe(true);

    cleanup();

    for (const entry of registered) {
      expect(entry.unlisten).toHaveBeenCalledTimes(1);
    }

    // A second call must not double-unregister (page unmount + app teardown).
    cleanup();
    for (const entry of registered) {
      expect(entry.unlisten).toHaveBeenCalledTimes(1);
    }
  });

  it('clears the log buffer without touching the snapshot', async () => {
    await start();
    emit(OCTOPAD_EVENTS.serialLog, { line: 'READY' });
    emit(OCTOPAD_EVENTS.serialLog, { line: 'INFO 1.4.2' });
    await flush();

    expect(get(logEntries)).toHaveLength(2);

    clearLogs();

    expect(get(logEntries)).toHaveLength(0);
    expect(get(appState)?.config.device).toBe('octopad');
  });
});

describe('event-driven state updates', () => {
  it('applies dirty, mode and selection events', async () => {
    await start();

    emit(OCTOPAD_EVENTS.dirtyChanged, { dirty: true });
    emit(OCTOPAD_EVENTS.modeChanged, { mode_index: 1 });
    emit(OCTOPAD_EVENTS.selectionChanged, { index: 3 });
    await flush();

    const snapshot = get(appState);
    expect(snapshot?.dirty).toBe(true);
    expect(snapshot?.modeIndex).toBe(1);
    expect(snapshot?.selectedIndex).toBe(3);
    // navigation is not an edit: the config itself is untouched by these events
    expect(snapshot?.config.modes[1].name).toBe('Home Assistant');
  });

  it('applies serial, BLE, device name and firmware events', async () => {
    await start();

    emit(OCTOPAD_EVENTS.serialStatusChanged, { connected: true, port: '/dev/ttyUSB0' });
    emit(OCTOPAD_EVENTS.bleStatusChanged, { connected: true, message: 'GATT ready' });
    emit(OCTOPAD_EVENTS.deviceNameChanged, { name: 'Studio Pad' });
    emit(OCTOPAD_EVENTS.deviceInfoChanged, { version: '1.4.2' });
    await flush();

    const snapshot = get(appState);
    expect(snapshot?.serial).toEqual({ connected: true, port: '/dev/ttyUSB0' });
    expect(snapshot?.ble).toEqual({ connected: true, message: 'GATT ready' });
    expect(snapshot?.device).toEqual({ name: 'Studio Pad', firmwareVersion: '1.4.2' });
  });

  it('clears an optional device name and firmware version when the core sends null', async () => {
    await start();

    emit(OCTOPAD_EVENTS.deviceNameChanged, { name: 'Studio Pad' });
    emit(OCTOPAD_EVENTS.deviceInfoChanged, { version: '1.4.2' });
    await flush();
    expect(get(appState)?.device.name).toBe('Studio Pad');

    emit(OCTOPAD_EVENTS.deviceNameChanged, { name: null });
    emit(OCTOPAD_EVENTS.deviceInfoChanged, { version: null });
    await flush();

    expect(get(appState)?.device).toEqual({ name: null, firmwareVersion: null });
  });

  it('applies Home Assistant and installed-app counters', async () => {
    await start();

    emit(OCTOPAD_EVENTS.haStatusChanged, { connected: true, message: 'connected' });
    emit(OCTOPAD_EVENTS.haEntitiesChanged, { count: 42 });
    emit(OCTOPAD_EVENTS.appsChanged, { count: 7 });
    await flush();

    expect(get(appState)?.homeAssistant).toEqual({
      connected: true,
      message: 'connected',
      entitiesCount: 42,
    });
    expect(get(appState)?.installedAppsCount).toBe(7);
  });

  it('applies timing events to the config', async () => {
    await start();

    emit(OCTOPAD_EVENTS.timingChanged, { field: 'debounceMs', value: 40 });
    emit(OCTOPAD_EVENTS.timingChanged, { field: 'clickTimeoutMs', value: 300 });
    emit(OCTOPAD_EVENTS.timingChanged, { field: 'holdTimeoutMs', value: 500 });
    await flush();

    expect(get(appState)?.config.debounceMs).toBe(40);
    expect(get(appState)?.config.clickTimeoutMs).toBe(300);
    expect(get(appState)?.config.holdTimeoutMs).toBe(500);
  });

  it('re-reads the snapshot when a button changes', async () => {
    await start();

    currentSnapshot.config.modes[0].buttons[1].label = 'Terminal';
    currentSnapshot.config.modes[0].buttons[1].single = { type: 'shortcut', key: 'k', modifiers: ['ctrl'] };
    currentSnapshot.dirty = true;

    emit(OCTOPAD_EVENTS.buttonChanged, { index: 1 });
    await flush();

    expect(invokeMock.mock.calls.filter(([command]) => command === 'get_state_snapshot')).toHaveLength(2);
    const button = get(appState)?.config.modes[0].buttons[1];
    expect(button?.label).toBe('Terminal');
    expect(button?.single).toEqual({ type: 'shortcut', key: 'k', modifiers: ['ctrl'] });
  });

  it('re-reads the snapshot when the whole config is replaced', async () => {
    await start();

    currentSnapshot.configPath = '/home/user/.config/octopad/imported.json';
    currentSnapshot.dirty = true;
    currentSnapshot.config.modes[0].buttons[0].label = 'Imported';

    emit(OCTOPAD_EVENTS.configReplaced, { source: 'import' });
    await flush();

    expect(get(appState)?.configPath).toBe('/home/user/.config/octopad/imported.json');
    expect(get(appState)?.dirty).toBe(true);
    expect(get(appState)?.config.modes[0].buttons[0].label).toBe('Imported');
  });

  it('records a preset slug from config-saved and refreshes the preset list', async () => {
    await start();

    emit(OCTOPAD_EVENTS.configSaved, { ok: true, preset: 'desk-set' });
    emit(OCTOPAD_EVENTS.presetsChanged, { name: 'desk-set' });
    await flush();

    expect(get(appState)?.activePreset).toBe('desk-set');
    expect(get(presets)).toEqual(['desk-set', 'gaming']);
  });

  it('appends serial log lines in arrival order', async () => {
    await start();

    emit(OCTOPAD_EVENTS.serialLog, { line: '[SERIAL] READY' });
    emit(OCTOPAD_EVENTS.serialLog, { line: '[TX] INFO' });
    await flush();

    expect(get(logEntries).map((entry) => entry.line)).toEqual(['[SERIAL] READY', '[TX] INFO']);
  });

  it('keeps only the newest log entries', async () => {
    await start();

    for (let index = 0; index < LOG_LIMIT + 25; index += 1) {
      emit(OCTOPAD_EVENTS.serialLog, { line: `line ${index}` });
    }
    await flush();

    const entries = get(logEntries);
    expect(entries).toHaveLength(LOG_LIMIT);
    expect(entries[0].line).toBe('line 25');
    expect(entries[entries.length - 1].line).toBe(`line ${LOG_LIMIT + 24}`);
  });
});

describe('payload tolerance', () => {
  it('ignores unknown extra payload fields', async () => {
    await start();
    const before = structuredClone(get(appState));

    emit(OCTOPAD_EVENTS.dirtyChanged, { dirty: true, source: 'ui', nested: { a: 1 } });
    emit(OCTOPAD_EVENTS.modeChanged, { mode_index: 1, unexpected: 'ignored' });
    emit(OCTOPAD_EVENTS.timingChanged, { field: 'clickTimeoutMs', value: 300, source: 'ui' });
    emit(OCTOPAD_EVENTS.serialLog, { line: 'READY', direction: 'rx', nope: 42 });
    emit(OCTOPAD_EVENTS.deviceNameChanged, { name: 'Studio Pad', id: 7 });
    await flush();

    const after = get(appState);
    expect(after?.dirty).toBe(true);
    expect(after?.modeIndex).toBe(1);
    expect(after?.config.clickTimeoutMs).toBe(300);
    expect(after?.device.name).toBe('Studio Pad');
    expect(get(logEntries)[0].line).toBe('READY');

    // No event may add, rename or drop a snapshot key, and no payload key may
    // leak into the config.
    expect(Object.keys(after ?? {}).slice().sort()).toEqual(Object.keys(before ?? {}).slice().sort());
    expect(after?.config).not.toHaveProperty('mode_index');
    expect(after?.config).not.toHaveProperty('source');
  });

  it('ignores payloads whose known fields have the wrong type', async () => {
    await start();

    emit(OCTOPAD_EVENTS.modeChanged, { mode_index: 'one' });
    emit(OCTOPAD_EVENTS.dirtyChanged, { dirty: 'yes' });
    emit(OCTOPAD_EVENTS.timingChanged, { field: 'unknownMs', value: 10 });
    emit(OCTOPAD_EVENTS.selectionChanged, { index: null });
    emit(OCTOPAD_EVENTS.serialLog, { line: 42 });
    await flush();

    const snapshot = get(appState);
    expect(snapshot?.modeIndex).toBe(0);
    expect(snapshot?.dirty).toBe(false);
    expect(snapshot?.selectedIndex).toBe(0);
    expect(snapshot?.config.debounceMs).toBe(25);
    expect(get(logEntries)).toHaveLength(0);
  });
});

describe('command failure surfacing', () => {
  it('reports a command that resolved to false without claiming success', async () => {
    await start();
    // queued after startup so the one-shot rejection lands on the command
    invokeMock.mockResolvedValueOnce(false);

    expect(await runCommand('set mode', () => setMode(1))).toBe(false);

    expect(get(lastError)).toMatch(/set mode/);
    expect(get(appState)?.modeIndex).toBe(0);
  });

  it('reports a rejected command and leaves local state untouched', async () => {
    await start();
    invokeMock.mockRejectedValueOnce(new Error('transport unavailable'));

    expect(await runCommand('send device command', () => sendDeviceCommand('INFO'))).toBe(false);

    expect(get(lastError)).toMatch(/transport unavailable/);
    expect(get(appState)?.config.modes).toHaveLength(2);
  });

  it('clears the surfaced error after the next successful command', async () => {
    await start();
    invokeMock.mockResolvedValueOnce(false);
    await runCommand('set mode', () => setMode(1));
    expect(get(lastError)).not.toBeNull();

    expect(await runCommand('set mode', () => setMode(1))).toBe(true);

    expect(get(lastError)).toBeNull();
  });

  it('surfaces a failing snapshot fetch instead of leaving a half-built store', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'get_state_snapshot') {
        return Promise.reject(new Error('ipc unavailable'));
      }
      return Promise.resolve([]);
    });

    await expect(initializeAppState()).rejects.toThrow(/ipc unavailable/);
    expect(get(appState)).toBeNull();
  });

  it('invokes commands with the documented snake_case names', async () => {
    await start();
    invokeMock.mockClear();

    await setMode(1);
    await setTiming('debounceMs', 40);
    await sendDeviceCommand('WHO');
    await getStateSnapshot();
    await listPresets();

    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'set_mode_cmd',
      'set_timing_cmd',
      'send_device_command_cmd',
      'get_state_snapshot',
      'list_presets_cmd',
    ]);
    expect(invokeMock.mock.calls[0][1]).toEqual({ index: 1 });
    expect(invokeMock.mock.calls[1][1]).toEqual({ field: 'debounceMs', value: 40 });
    expect(invokeMock.mock.calls[2][1]).toEqual({ command: 'WHO' });
  });
});