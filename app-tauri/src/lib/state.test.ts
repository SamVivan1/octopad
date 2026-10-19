import { get } from 'svelte/store';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

import { sendDeviceCommand } from './api';
import { APP_EVENT_NAMES } from './api';
import { appState, initializeAppState } from './state';
import type { AppSnapshot } from './types';

function snapshot(): AppSnapshot {
  return {
    config: {
      version: 2.6,
      schemaVersion: 1,
      device: 'OctoPad',
      debounceMs: 25,
      clickTimeoutMs: 250,
      holdTimeoutMs: 450,
      activeModeIndex: 0,
      modes: []
    },
    configPath: '/tmp/OctoPad/config.json',
    modeIndex: 0,
    selectedIndex: 0,
    dirty: false,
    activePreset: null,
    serial: { connected: false, port: null },
    ble: { connected: false, message: '' },
    device: { name: null, firmwareVersion: null },
    homeAssistant: { connected: false, message: '', entitiesCount: 0 },
    installedAppsCount: 0
  };
}

describe('frontend app state bridge', () => {
  let eventHandlers: Map<string, (event: { payload: unknown }) => void>;
  let unlisteners: Array<ReturnType<typeof vi.fn>>;
  let order: string[];

  beforeEach(() => {
    eventHandlers = new Map();
    unlisteners = [];
    order = [];
    appState.set({ snapshot: null, logs: [] });
    invokeMock.mockReset();
    listenMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      order.push(command);
      return snapshot();
    });
    listenMock.mockImplementation(
      async (name: string, handler: (event: { payload: unknown }) => void) => {
        order.push(name);
        eventHandlers.set(name, handler);
        const unlisten = vi.fn();
        unlisteners.push(unlisten);
        return unlisten;
      }
    );
  });

  it('registers all listeners before loading the initial snapshot', async () => {
    await initializeAppState();

    expect(order).toEqual([
      ...APP_EVENT_NAMES,
      'get_state_snapshot'
    ]);
    expect(get(appState).snapshot).toEqual(snapshot());
  });

  it('applies recognized mode, selection, dirty, and connection fields only', async () => {
    await initializeAppState();

    eventHandlers.get('octopad://mode-changed')?.({
      payload: { mode_index: 1, unrelated: 'ignored' }
    });
    eventHandlers.get('octopad://selection-changed')?.({
      payload: { index: 3 }
    });
    eventHandlers.get('octopad://dirty-changed')?.({
      payload: { dirty: true, config: { corrupted: true } }
    });
    eventHandlers.get('octopad://serial-status-changed')?.({
      payload: { connected: true, port: '/dev/ttyUSB0' }
    });
    eventHandlers.get('octopad://ble-status-changed')?.({
      payload: { connected: true, message: 'ready' }
    });

    const state = get(appState);
    expect(state.snapshot).toMatchObject({
      modeIndex: 1,
      selectedIndex: 3,
      dirty: true,
      serial: { connected: true, port: '/dev/ttyUSB0' },
      ble: { connected: true, message: 'ready' }
    });
    expect(state.snapshot?.config).toMatchObject({
      ...snapshot().config,
      activeModeIndex: 1
    });
  });

  it('propagates invoke rejection from device commands', async () => {
    const failure = new Error('transport write failed');
    invokeMock.mockRejectedValueOnce(failure);

    await expect(sendDeviceCommand('WHO')).rejects.toBe(failure);
  });

  it('unregisters every listener during cleanup', async () => {
    const cleanup = await initializeAppState();

    cleanup();

    expect(unlisteners).toHaveLength(APP_EVENT_NAMES.length);
    for (const unlisten of unlisteners) {
      expect(unlisten).toHaveBeenCalledOnce();
    }
  });
});
