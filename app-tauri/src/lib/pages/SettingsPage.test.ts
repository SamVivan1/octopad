// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { appState } from '../state';
import type { AppSnapshot } from '../types';

const invokeMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

import AppShell from '../components/AppShell.svelte';
import SettingsPage from './SettingsPage.svelte';
import ProfilesPage from './ProfilesPage.svelte';

function makeSnapshot(): AppSnapshot {
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
    ble: { connected: false, message: 'Bluetooth unavailable' },
    device: { name: 'My Pad', firmwareVersion: '2.6' },
    homeAssistant: { connected: false, message: '', entitiesCount: 0 },
    installedAppsCount: 0
  };
}

describe('Settings and Profiles pages', () => {
  let wireState: AppSnapshot;
  let renameResult: boolean;
  let timingResult: boolean;
  let saveResult: boolean;
  let importFailure: Error | undefined;

  beforeEach(() => {
    wireState = makeSnapshot();
    renameResult = true;
    timingResult = true;
    saveResult = true;
    importFailure = undefined;
    appState.set({ snapshot: wireState, logs: [] });
    invokeMock.mockReset();
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => undefined);
    invokeMock.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === 'get_state_snapshot') {
        return Promise.resolve({ ...wireState, config: { ...wireState.config } });
      }
      if (command === 'set_timing_cmd') {
        if (timingResult) {
          const field = args?.field as 'debounceMs' | 'clickTimeoutMs' | 'holdTimeoutMs';
          wireState.config[field] = args?.value as number;
          wireState.dirty = true;
        }
        return Promise.resolve(timingResult);
      }
      if (command === 'rename_device_cmd') {
        if (renameResult) wireState.device.name = args?.name as string;
        return Promise.resolve(renameResult);
      }
      if (command === 'save_config_cmd') {
        if (saveResult) wireState.dirty = false;
        return Promise.resolve(saveResult);
      }
      if (command === 'import_legacy_config') {
        if (importFailure) return Promise.reject(importFailure);
        wireState.config = { ...wireState.config, device: 'Imported Pad' };
        wireState.dirty = true;
        return Promise.resolve({ source: 'legacy' });
      }
      return Promise.resolve(true);
    });
  });

  afterEach(() => cleanup());

  it('shows timing bounds and device transport status', () => {
    render(SettingsPage);

    const debounce = screen.getByRole('spinbutton', { name: 'Debounce (ms)' }) as HTMLInputElement;
    const click = screen.getByRole('spinbutton', { name: 'Click timeout (ms)' }) as HTMLInputElement;
    const hold = screen.getByRole('spinbutton', { name: 'Hold timeout (ms)' }) as HTMLInputElement;
    expect([debounce.min, debounce.max, debounce.step]).toEqual(['0', '1000', '5']);
    expect([click.min, click.max, click.step]).toEqual(['50', '2000', '5']);
    expect([hold.min, hold.max, hold.step]).toEqual(['50', '3000', '5']);
    expect(screen.getByText('My Pad')).toBeTruthy();
    expect(screen.getByText('2.6')).toBeTruthy();
    expect(screen.getByText('Serial')).toBeTruthy();
    expect(screen.getByText('/dev/ttyUSB0')).toBeTruthy();
  });

  it('rejects timings outside the range or step without invoking the command', async () => {
    render(SettingsPage);

    await fireEvent.change(screen.getByRole('spinbutton', { name: 'Click timeout (ms)' }), {
      target: { value: '49' }
    });
    expect(screen.getByRole('alert').textContent).toContain('50 and 2000');
    expect(invokeMock).not.toHaveBeenCalledWith('set_timing_cmd', expect.anything());

    await fireEvent.change(screen.getByRole('spinbutton', { name: 'Debounce (ms)' }), {
      target: { value: '22' }
    });
    expect(screen.getByRole('alert').textContent).toContain('increments of 5');
    expect(invokeMock).not.toHaveBeenCalledWith('set_timing_cmd', expect.anything());
  });

  it('sends valid timing changes to the state command', async () => {
    render(SettingsPage);

    await fireEvent.change(screen.getByRole('spinbutton', { name: 'Debounce (ms)' }), {
      target: { value: '30' }
    });
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_timing_cmd', {
        field: 'debounceMs',
        value: 30
      })
    );
    expect(wireState.config.debounceMs).toBe(30);
  });

  it('trims device names and accepts exactly 24 characters', async () => {
    render(SettingsPage);
    const name = 'A'.repeat(24);

    await fireEvent.input(screen.getByRole('textbox', { name: 'Device name' }), {
      target: { value: `  ${name}  ` }
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Rename device' }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('rename_device_cmd', { name })
    );
    expect(wireState.device.name).toBe(name);
  });

  it('rejects a device name longer than 24 characters after trimming', async () => {
    render(SettingsPage);

    await fireEvent.input(screen.getByRole('textbox', { name: 'Device name' }), {
      target: { value: ` ${'N'.repeat(25)} ` }
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Rename device' }));

    expect(screen.getByRole('alert').textContent).toContain('1–24 characters');
    expect(invokeMock).not.toHaveBeenCalledWith('rename_device_cmd', expect.anything());
    expect(wireState.device.name).toBe('My Pad');
  });

  it('surfaces failed device renames without claiming a new device name', async () => {
    renameResult = false;
    render(SettingsPage);

    await fireEvent.input(screen.getByRole('textbox', { name: 'Device name' }), {
      target: { value: 'New Pad' }
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Rename device' }));

    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(screen.getByRole('alert').textContent).toContain('Rename failed');
    expect(wireState.device.name).toBe('My Pad');
  });

  it('imports a legacy config into the working snapshot without saving it', async () => {
    render(ProfilesPage);

    await fireEvent.click(screen.getByRole('button', { name: /Import legacy config/ }));
    expect(await screen.findByRole('status')).toBeTruthy();
    expect(get(appState).snapshot?.config.device).toBe('Imported Pad');
    expect(get(appState).snapshot?.dirty).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith('import_legacy_config');
    expect(invokeMock).not.toHaveBeenCalledWith('save_config_cmd');
  });

  it('shows legacy import errors and leaves the working snapshot unchanged', async () => {
    importFailure = new Error('legacy file could not be read');
    render(ProfilesPage);

    await fireEvent.click(screen.getByRole('button', { name: /Import legacy config/ }));
    expect(await screen.findByRole('status')).toBeTruthy();
    expect(screen.getByRole('status').textContent).toContain('Import failed');
    expect(get(appState).snapshot?.config.device).toBe('OctoPad');
    expect(invokeMock).not.toHaveBeenCalledWith('save_config_cmd');
  });

  it('saves config and refreshes the dirty state', async () => {
    render(ProfilesPage);
    expect(screen.getByText('Unsaved changes')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('All changes saved')).toBeTruthy();
    expect(invokeMock).toHaveBeenCalledWith('save_config_cmd');
  });

  it('opens the functional Settings page from the application navigation', async () => {
    render(AppShell);
    await screen.findByText('My Pad');
    await fireEvent.click(screen.getByRole('button', { name: 'Settings' }));

    expect(screen.getByRole('spinbutton', { name: 'Debounce (ms)' })).toBeTruthy();
  });
});
