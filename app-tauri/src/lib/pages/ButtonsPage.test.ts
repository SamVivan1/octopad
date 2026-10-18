// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { appState } from '../state';
import type { AppSnapshot, Button } from '../types';

const invokeMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));

import ButtonsPage from './ButtonsPage.svelte';

function makeButton(mode: string, index: number): Button {
  const action = { type: 'none' };
  return {
    index,
    pin: index,
    label: `${mode} Button ${index + 1}`,
    single: action,
    double: [{ type: 'text' }, { type: 'media' }],
    hold: action
  };
}

function makeSnapshot(): AppSnapshot {
  const desktop = {
    id: 'desktop',
    name: 'Desktop Mode',
    ledState: true,
    buttons: Array.from({ length: 8 }, (_, index) => makeButton('Desktop', index))
  };
  const homeAssistant = {
    id: 'ha',
    name: 'Home Assistant Mode',
    ledState: false,
    buttons: Array.from({ length: 8 }, (_, index) => makeButton('Home Assistant', index))
  };
  return {
    config: {
      version: 2.6,
      schemaVersion: 1,
      device: 'OctoPad',
      debounceMs: 25,
      clickTimeoutMs: 250,
      holdTimeoutMs: 450,
      activeModeIndex: 0,
      modes: [desktop, homeAssistant]
    },
    configPath: '/tmp/OctoPad/config.json',
    modeIndex: 0,
    selectedIndex: 2,
    dirty: false,
    activePreset: null,
    serial: { connected: false, port: null },
    ble: { connected: false, message: '' },
    device: { name: null, firmwareVersion: null },
    homeAssistant: { connected: false, message: '', entitiesCount: 0 },
    installedAppsCount: 0
  };
}

describe('ButtonsPage', () => {
  let wireState: AppSnapshot;
  let commandOrder: string[];
  let resolveSelection: ((value: boolean) => void) | undefined;

  beforeEach(() => {
    wireState = makeSnapshot();
    commandOrder = [];
    resolveSelection = undefined;
    appState.set({ snapshot: wireState, logs: [] });
    invokeMock.mockReset();
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => undefined);
    invokeMock.mockImplementation((command: string, args?: Record<string, unknown>) => {
      commandOrder.push(command);
      if (command === 'get_state_snapshot') {
        return Promise.resolve({
          ...wireState,
          config: { ...wireState.config, modes: wireState.config.modes }
        });
      }
      if (command === 'set_mode_cmd') {
        const index = args?.index as number;
        wireState.modeIndex = index;
        wireState.config.activeModeIndex = index;
        return Promise.resolve(true);
      }
      if (command === 'select_button_cmd') {
        wireState.selectedIndex = args?.index as number;
        return new Promise<boolean>((resolve) => {
          resolveSelection = resolve;
        });
      }
      if (command === 'set_label_cmd') {
        const mode = wireState.config.modes[wireState.modeIndex];
        const button = mode.buttons[wireState.selectedIndex];
        button.label = args?.text as string;
        return Promise.resolve();
      }
      return Promise.resolve(true);
    });
  });

  afterEach(() => cleanup());

  it('shows eight labeled key cards and tracks the selected key in each mode', async () => {
    render(ButtonsPage);

    let grid = screen.getByRole('group', { name: 'Desktop Mode buttons' });
    expect(within(grid).getAllByRole('button', { name: /^Key / })).toHaveLength(8);
    const selectedDesktopKey = within(grid).getByRole('button', {
      name: 'Key 3, Desktop Button 3'
    });
    expect(selectedDesktopKey.getAttribute('aria-pressed')).toBe('true');
    expect(within(selectedDesktopKey).getByText('S 1')).toBeTruthy();
    expect(within(selectedDesktopKey).getByText('D 2')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Home Assistant' }));
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Home Assistant' }).hasAttribute('disabled')
      ).toBe(false)
    );
    grid = screen.getByRole('group', { name: 'Home Assistant Mode buttons' });
    expect(within(grid).getAllByRole('button', { name: /^Key / })).toHaveLength(8);
    expect(
      within(grid)
        .getByRole('button', { name: 'Key 3, Home Assistant Button 3' })
        .getAttribute('aria-pressed')
    ).toBe('true');
    expect(commandOrder).toContain('set_mode_cmd');
  });

  it('awaits key selection before enabling edits for the newly selected key', async () => {
    render(ButtonsPage);
    const label = screen.getByRole('textbox', { name: 'Current key label' });

    await fireEvent.click(screen.getByRole('button', { name: 'Key 5, Desktop Button 5' }));
    expect(label.hasAttribute('disabled')).toBe(true);
    expect(commandOrder).toEqual(['select_button_cmd']);

    resolveSelection?.(true);
    await waitFor(() => expect(label.hasAttribute('disabled')).toBe(false));

    await fireEvent.change(label, { target: { value: 'Updated key' } });
    await waitFor(() => expect(commandOrder).toEqual([
      'select_button_cmd',
      'get_state_snapshot',
      'set_label_cmd',
      'get_state_snapshot'
    ]));
    expect(wireState.config.modes[0].buttons[4].label).toBe('Updated key');
  });
});
