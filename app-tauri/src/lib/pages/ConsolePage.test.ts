// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { appState, initializeAppState } from '../state';
import type { AppSnapshot, TimestampedLogLine } from '../types';

const invokeMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());
const eventHandlers = vi.hoisted(
  () => new Map<string, (event: { payload: unknown }) => void>()
);

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

import ConsoleLog from '../components/ConsoleLog.svelte';
import AppShell from '../components/AppShell.svelte';
import ConsolePage from './ConsolePage.svelte';

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

describe('Serial Console', () => {
  let stopListening: (() => void) | undefined;
  let sendResult: boolean;

  beforeEach(() => {
    stopListening = undefined;
    sendResult = true;
    eventHandlers.clear();
    appState.set({ snapshot: makeSnapshot(), logs: [] });
    invokeMock.mockReset();
    listenMock.mockReset();
    invokeMock.mockImplementation((command: string) =>
      command === 'get_state_snapshot' ? Promise.resolve(makeSnapshot()) : Promise.resolve(sendResult)
    );
    listenMock.mockImplementation(
      async (name: string, handler: (event: { payload: unknown }) => void) => {
        eventHandlers.set(name, handler);
        return vi.fn();
      }
    );
  });

  afterEach(() => {
    stopListening?.();
    cleanup();
  });

  it('appends serial-log events to the shared store and renders classified timestamps', async () => {
    stopListening = await initializeAppState();
    render(ConsolePage);

    eventHandlers.get('octopad://serial-log')?.({ payload: { line: '[ERROR] write failed' } });

    const line = await screen.findByText('[ERROR] write failed');
    expect(line.closest('.log-line')?.classList.contains('error')).toBe(true);
    expect(line.closest('.log-line')?.querySelector('time')?.textContent).toMatch(/^\d{2}:\d{2}:\d{2}$/);
    expect(get(appState).logs).toHaveLength(1);
  });

  it('classifies transmit, action, error, and system lines', () => {
    const logs: TimestampedLogLine[] = [
      { line: '[TX] WHO', timestamp: 0 },
      { line: '[TX-BLE] INFO', timestamp: 1000 },
      { line: '[ACTION] Button 1', timestamp: 2000 },
      { line: '[ERROR] disconnected', timestamp: 3000 },
      { line: '[HARDWARE] Connected', timestamp: 4000 }
    ];
    render(ConsoleLog, { props: { logs, onClear: vi.fn() } });

    for (const [text, kind] of [
      ['[TX] WHO', 'tx'],
      ['[TX-BLE] INFO', 'tx'],
      ['[ACTION] Button 1', 'action'],
      ['[ERROR] disconnected', 'error'],
      ['[HARDWARE] Connected', 'system']
    ]) {
      expect(screen.getByText(text).closest('.log-line')?.classList.contains(kind)).toBe(true);
    }
  });

  it('sends each quick command with its exact command value', async () => {
    render(ConsolePage);

    for (const command of ['WHO', 'INFO', 'SAVE', 'HELP']) {
      await fireEvent.click(screen.getByRole('button', { name: command }));
      await waitFor(() =>
        expect(invokeMock).toHaveBeenLastCalledWith('send_device_command_cmd', { command })
      );
    }
  });

  it('sends the entered command on Enter and clears it only after success', async () => {
    render(ConsolePage);
    const input = screen.getByRole('textbox', { name: 'Command' }) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: '  CUSTOM  ' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('send_device_command_cmd', { command: 'CUSTOM' })
    );
    expect(input.value).toBe('');
  });

  it('keeps command text and shows an error when the device write fails', async () => {
    sendResult = false;
    render(ConsolePage);
    const input = screen.getByRole('textbox', { name: 'Command' }) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'CUSTOM' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(input.value).toBe('CUSTOM');
  });

  it('clears the shared log list', async () => {
    appState.set({
      snapshot: makeSnapshot(),
      logs: [
        { line: '[RX] one', timestamp: 1 },
        { line: '[RX] two', timestamp: 2 }
      ]
    });
    render(ConsolePage);

    await fireEvent.click(screen.getByRole('button', { name: 'Clear' }));

    expect(get(appState).logs).toEqual([]);
    expect(screen.queryByText('[RX] one')).toBeNull();
  });

  it('trims 200 oldest lines after the visible log exceeds 1500 entries', async () => {
    stopListening = await initializeAppState();
    const emit = eventHandlers.get('octopad://serial-log');
    expect(emit).toBeTruthy();

    for (let index = 0; index <= 1500; index += 1) {
      emit?.({ payload: { line: `line-${index}` } });
    }

    const logs = get(appState).logs;
    expect(logs).toHaveLength(1301);
    expect(logs[0].line).toBe('line-200');
    expect(logs.at(-1)?.line).toBe('line-1500');
  });

  it('opens the live console from application navigation', async () => {
    render(AppShell);
    await screen.findByText('OctoPad');
    await fireEvent.click(screen.getByRole('button', { name: 'Serial Console' }));

    expect(screen.getByRole('textbox', { name: 'Command' })).toBeTruthy();
  });
});
