// @vitest-environment jsdom
/**
 * Contract tests for the app shell (`AppShell.svelte` and the pages it renders).
 *
 * These are the only tests that need a DOM, so this file opts into jsdom with the
 * `@vitest-environment` pragma instead of changing the global Vitest environment:
 * `state.test.ts` keeps running in plain node with no DOM at all.
 *
 * Every `@tauri-apps` call is mocked, so the suite exercises the real wiring the
 * M2 Review Focus cares about: listeners are registered before the snapshot is
 * applied, the header renders what the core reports, and unmounting the shell
 * unregisters every listener exactly once.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

import App from '../../App.svelte';
import AppShell from './AppShell.svelte';
import { resetAppState } from '../state';
import type { AppSnapshot, Button } from '../types';

// -- fixtures ----------------------------------------------------------------

const PINOUT = [20, 9, 2, 1, 21, 10, 3, 0];

function makeButton(index: number): Button {
  return {
    index,
    pin: PINOUT[index],
    label: `Button ${index + 1}`,
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
          buttons: Array.from({ length: 8 }, (_unused, index) => makeButton(index)),
        },
        {
          id: 'ha',
          name: 'Home Assistant',
          ledState: false,
          buttons: Array.from({ length: 8 }, (_unused, index) => makeButton(index)),
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

interface Registration {
  name: string;
  unlisten: ReturnType<typeof vi.fn>;
}

let handlers = new Map<string, (event: { payload: unknown }) => void>();
let registered: Registration[] = [];
let currentSnapshot: AppSnapshot = makeSnapshot();

function invokeDefault(command: string): unknown {
  if (command === 'get_state_snapshot') {
    return Promise.resolve(structuredClone(currentSnapshot));
  }
  if (command === 'list_presets_cmd') {
    return Promise.resolve([]);
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

function text(testId: string): string {
  return screen.getByTestId(testId).textContent?.trim() ?? '';
}

function sidebar(): HTMLElement {
  return screen.getByRole('navigation', { name: 'Primary' });
}

function activePage(): string {
  return screen.getByTestId('active-page').getAttribute('data-page') ?? '';
}

async function navigateTo(label: string): Promise<void> {
  await fireEvent.click(within(sidebar()).getByRole('button', { name: label }));
}

beforeEach(() => {
  handlers = new Map();
  registered = [];
  currentSnapshot = makeSnapshot();
  invokeMock.mockReset();
  listenMock.mockReset();
  invokeMock.mockImplementation((command: string) => invokeDefault(command));
  listenMock.mockImplementation(async (name: string, handler: (event: { payload: unknown }) => void) => {
    const unlisten = vi.fn();
    registered.push({ name, unlisten });
    handlers.set(name, handler);
    return unlisten;
  });
  resetAppState();
});

afterEach(() => {
  cleanup();
  resetAppState();
});

// -- tests -------------------------------------------------------------------

describe('page navigation', () => {
  it('renders the four top-level pages in order', async () => {
    render(AppShell);
    await flush();

    const items = within(sidebar()).getAllByRole('button');
    expect(items.map((item) => item.textContent?.trim())).toEqual([
      'Buttons',
      'Serial Console',
      'Profiles',
      'Settings',
    ]);

    // Exactly one page is mounted, and it is the first one.
    expect(screen.getAllByTestId('active-page')).toHaveLength(1);
    expect(activePage()).toBe('buttons');
    expect(screen.getByRole('heading', { name: 'Buttons' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Buttons' }).getAttribute('aria-current')).toBe('page');
  });

  it('switches the visible page when the active page changes', async () => {
    render(AppShell);
    await flush();

    await navigateTo('Serial Console');
    expect(activePage()).toBe('console');
    expect(screen.getByRole('heading', { name: 'Serial Console' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'Buttons' })).toBeNull();

    await navigateTo('Profiles');
    expect(activePage()).toBe('profiles');
    expect(screen.getByRole('heading', { name: 'Profiles' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'Serial Console' })).toBeNull();

    await navigateTo('Settings');
    expect(activePage()).toBe('settings');
    expect(screen.getByRole('heading', { name: 'Settings' })).toBeTruthy();

    await navigateTo('Buttons');
    expect(activePage()).toBe('buttons');
  });

  it('shows the working config path on the Profiles page', async () => {
    currentSnapshot.configPath = '/home/user/.config/octopad/working.json';
    render(AppShell);
    await flush();

    await navigateTo('Profiles');

    expect(text('profiles-config-path')).toBe('/home/user/.config/octopad/working.json');
    expect(text('profiles-config-state')).toBe('Saved');
  });
});

describe('status header', () => {
  it('shows the saved state and both disconnected transports', async () => {
    render(AppShell);
    await flush();

    expect(text('status-config-state')).toBe('Saved');
    expect(text('status-serial')).toBe('Serial: Disconnected');
    expect(text('status-ble')).toBe('Bluetooth: Disconnected');
    expect(text('status-device')).toBe('Device: Unnamed');
    expect(text('status-firmware')).toBe('Firmware: unknown');
  });

  it('follows core dirty, serial, BLE, device name and firmware events', async () => {
    render(AppShell);
    await flush();

    emit('octopad://dirty-changed', { dirty: true });
    emit('octopad://serial-status-changed', { connected: true, port: '/dev/ttyUSB0' });
    emit('octopad://ble-status-changed', { connected: true, message: 'GATT ready' });
    emit('octopad://device-name-changed', { name: 'Studio Pad' });
    emit('octopad://device-info-changed', { version: '1.4.2' });
    await flush();

    expect(text('status-config-state')).toBe('Unsaved changes');
    expect(text('status-serial')).toBe('Serial: Connected (/dev/ttyUSB0)');
    expect(text('status-ble')).toBe('Bluetooth: Connected');
    expect(text('status-device')).toBe('Device: Studio Pad');
    expect(text('status-firmware')).toBe('Firmware: 1.4.2');

    emit('octopad://dirty-changed', { dirty: false });
    emit('octopad://serial-status-changed', { connected: false, port: null });
    emit('octopad://ble-status-changed', { connected: false, message: '' });
    await flush();

    expect(text('status-config-state')).toBe('Saved');
    expect(text('status-serial')).toBe('Serial: Disconnected');
    expect(text('status-ble')).toBe('Bluetooth: Disconnected');
  });

  it('surfaces a rejected command instead of claiming success', async () => {
    render(AppShell);
    await flush();

    invokeMock.mockRejectedValueOnce(new Error('ipc unavailable'));

    await navigateTo('Profiles');
    await fireEvent.click(screen.getByRole('button', { name: 'Save configuration' }));
    await flush();

    expect(screen.getByRole('alert').textContent).toMatch(/ipc unavailable/);
    expect(text('profiles-config-state')).toBe('Saved');
  });
});

describe('lifecycle', () => {
  it('unregisters every listener exactly once when the shell unmounts', async () => {
    const view = render(AppShell);
    await flush();

    expect(registered.length).toBeGreaterThan(0);
    expect(registered.every((entry) => entry.unlisten.mock.calls.length === 0)).toBe(true);

    view.unmount();

    expect(registered.length).toBeGreaterThan(0);
    for (const entry of registered) {
      expect(entry.unlisten, `unlisten for ${entry.name}`).toHaveBeenCalledTimes(1);
    }
  });

  it('registers listeners before the first snapshot is applied', async () => {
    const order: string[] = [];
    invokeMock.mockImplementation((command: string) => {
      order.push(`invoke:${command}`);
      return invokeDefault(command);
    });
    listenMock.mockImplementation(
      async (name: string, handler: (event: { payload: unknown }) => void) => {
        order.push(`listen:${name}`);
        const unlisten = vi.fn();
        registered.push({ name, unlisten });
        handlers.set(name, handler);
        return unlisten;
      }
    );

    render(AppShell);
    await flush();

    const firstInvoke = order.findIndex((entry) => entry.startsWith('invoke:'));
    const lastListen = order.map((entry) => entry.startsWith('listen:')).lastIndexOf(true);
    expect(firstInvoke).toBeGreaterThan(lastListen);
  });

  it('mounts the same shell from the app entry point', async () => {
    render(App);
    await flush();

    expect(within(sidebar()).getAllByRole('button')).toHaveLength(4);
    expect(activePage()).toBe('buttons');
    expect(text('status-config-state')).toBe('Saved');
  });
});