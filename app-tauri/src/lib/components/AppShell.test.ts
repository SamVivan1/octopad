// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());
const unlisteners = vi.hoisted(() => [] as Array<ReturnType<typeof vi.fn>>);

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

import AppShell from './AppShell.svelte';
import { appState } from '../state';

function snapshot() {
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
    configPath: '/home/test/.config/OctoPad/config.json',
    modeIndex: 0,
    selectedIndex: 0,
    dirty: true,
    activePreset: null,
    serial: { connected: true, port: '/dev/ttyUSB0' },
    ble: { connected: false, message: '' },
    device: { name: 'My Pad', firmwareVersion: '2.6' },
    homeAssistant: { connected: false, message: '', entitiesCount: 0 },
    installedAppsCount: 0
  };
}

describe('AppShell', () => {
  beforeEach(() => {
    unlisteners.length = 0;
    appState.set({ snapshot: null, logs: [] });
    invokeMock.mockReset();
    listenMock.mockReset();
    invokeMock.mockResolvedValue(snapshot());
    listenMock.mockImplementation(async () => {
      const unlisten = vi.fn();
      unlisteners.push(unlisten);
      return unlisten;
    });
  });

  afterEach(() => cleanup());

  it('shows the four pages in order and navigates to the active page', async () => {
    render(AppShell);
    await screen.findByText('My Pad');

    const navigation = await screen.findByRole('navigation', { name: 'Main navigation' });
    expect(
      within(navigation)
        .getAllByRole('button')
        .map((button) => button.textContent?.trim())
    ).toEqual(['Buttons', 'Serial Console', 'Profiles', 'Settings']);

    await fireEvent.click(within(navigation).getByRole('button', { name: 'Serial Console' }));
    expect(screen.getByRole('heading', { name: 'Serial Console' })).toBeTruthy();
    expect(
      within(navigation)
        .getByRole('button', { name: 'Serial Console' })
        .getAttribute('aria-current')
    ).toBe('page');
  });

  it('shows dirty state, device identity, and connection status in the header', async () => {
    render(AppShell);

    expect(await screen.findByText('Unsaved changes')).toBeTruthy();
    expect(screen.getByText('Serial connected')).toBeTruthy();
    expect(screen.getByText('BLE disconnected')).toBeTruthy();
    expect(screen.getByText('My Pad')).toBeTruthy();
    expect(screen.getByText('Firmware v2.6')).toBeTruthy();
  });

  it('unsubscribes UI event listeners when unmounted', async () => {
    const { unmount } = render(AppShell);
    await screen.findByText('My Pad');

    unmount();

    expect(unlisteners).toHaveLength(10);
    for (const unlisten of unlisteners) {
      expect(unlisten).toHaveBeenCalledOnce();
    }
  });
});
