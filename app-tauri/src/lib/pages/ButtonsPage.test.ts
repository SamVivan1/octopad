// @vitest-environment jsdom
/**
 * Contract tests for the Buttons page — the 8-key grid, the Desktop / Home
 * Assistant mode switch, and the selection safety rules the M2 Review Focus
 * calls out.
 *
 * What these tests pin down, beyond "8 cards render":
 *
 * - **The grid is a view, not a second copy of the config.** Labels, selection
 *   and mode always come from the shared snapshot, which is only updated by the
 *   core's own events. A click that the core never confirmed must not move the
 *   highlight, and a `button-changed` event must pull the new label through.
 * - **Commands are ordered.** A card click issues `select_button_cmd` and waits
 *   for it, so an edit queued right after a fast click can never be applied to
 *   the previously selected key.
 * - **Selection is exposed accessibly**, and the layout contract (4 columns,
 *   never fewer than 2) is pinned as data the stylesheet consumes.
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

import ButtonsPage from './ButtonsPage.svelte';
import { appState, initializeAppState, resetAppState } from '../state';
import type { AppSnapshot, Button, Mode } from '../types';

// -- fixtures ----------------------------------------------------------------

const PINOUT = [20, 9, 2, 1, 21, 10, 3, 0];

const DESKTOP_LABELS = [
  'Terminal',
  'Browser',
  'Mail',
  'Chat',
  'Music',
  'Focus',
  'Stream',
  'Sleep',
];

const HA_LABELS = [
  'Hall Light',
  'Desk Light',
  'Porch Light',
  'Music',
  'Movie Scene',
  'Good Morning',
  'Good Night',
  'Alarm',
];

function makeButton(index: number, label: string): Button {
  return {
    index,
    pin: PINOUT[index],
    label,
    // Legacy single-object slots, except key 1's double trigger which uses the
    // list shape: both must read as "configured" on the card.
    single: index === 0 ? { type: 'shortcut', key: 'F1', ctrl: true } : { type: 'none' },
    double: index === 0 ? [{ type: 'media', command: 'play_pause' }] : { type: 'none' },
    hold: index === 0 ? { type: 'mode_toggle' } : { type: 'none' },
  };
}

function makeMode(id: string, name: string, labels: string[]): Mode {
  return {
    id,
    name,
    ledState: false,
    buttons: labels.map((label, index) => makeButton(index, label)),
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
      modes: [makeMode('desktop', 'Desktop', DESKTOP_LABELS), makeMode('ha', 'Home Assistant', HA_LABELS)],
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

let handlers = new Map<string, EventHandler>();
let disposers: Array<() => void> = [];
let commandLog: string[] = [];
let currentSnapshot: AppSnapshot = makeSnapshot();

function invokeDefault(command: string): unknown {
  commandLog.push(command);
  if (command === 'get_state_snapshot') {
    return Promise.resolve(structuredClone(currentSnapshot));
  }
  if (command === 'list_presets_cmd') {
    return Promise.resolve([]);
  }
  return Promise.resolve(true);
}

async function flush(): Promise<void> {
  for (let tick = 0; tick < 5; tick += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

function emit(eventName: string, payload: unknown): void {
  const handler = handlers.get(eventName);
  expect(handler, `no listener registered for ${eventName}`).toBeTypeOf('function');
  handler?.({ payload });
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function cards(): HTMLElement[] {
  return screen.getAllByTestId('key-card');
}

function cardLabels(): string[] {
  return cards().map((card) => within(card).getByTestId('key-card-label').textContent?.trim() ?? '');
}

function cardNumbers(): string[] {
  return cards().map((card) => within(card).getByTestId('key-card-number').textContent?.trim() ?? '');
}

function selectedCardKeys(): string[] {
  return cards()
    .filter((card) => card.getAttribute('aria-pressed') === 'true')
    .map((card) => card.getAttribute('data-key') ?? '');
}

/** `runEdit` is the page's exported hook for every edit command. */
interface EditorHandle {
  runEdit<T>(command: () => Promise<T>): Promise<T>;
}

function editorOf(view: { component: unknown }): EditorHandle {
  return view.component as EditorHandle;
}

async function renderPage() {
  const view = render(ButtonsPage);
  await flush();
  return view;
}

beforeEach(async () => {
  handlers = new Map();
  disposers = [];
  commandLog = [];
  currentSnapshot = makeSnapshot();
  invokeMock.mockReset();
  listenMock.mockReset();
  invokeMock.mockImplementation((command: string) => invokeDefault(command));
  listenMock.mockImplementation(async (name: string, handler: EventHandler) => {
    const unlisten = vi.fn();
    handlers.set(name, handler);
    return unlisten;
  });
  resetAppState();
  disposers.push(await initializeAppState());
});

afterEach(() => {
  cleanup();
  for (const dispose of disposers) dispose();
  disposers = [];
  resetAppState();
});

// -- the eight physical keys -------------------------------------------------

describe('key grid', () => {
  it('renders all eight physical keys with their number and label in Desktop mode', async () => {
    await renderPage();

    expect(cards()).toHaveLength(8);
    expect(cardNumbers()).toEqual(['1', '2', '3', '4', '5', '6', '7', '8']);
    expect(cardLabels()).toEqual(DESKTOP_LABELS);
    expect(cards().map((card) => card.getAttribute('data-key'))).toEqual([
      '0',
      '1',
      '2',
      '3',
      '4',
      '5',
      '6',
      '7',
    ]);
  });

  it('shows a single, double and hold indicator on every card', async () => {
    await renderPage();

    for (const card of cards()) {
      const indicators = within(card).getAllByTestId('key-card-trigger');
      expect(indicators.map((pill) => pill.getAttribute('data-trigger'))).toEqual([
        'single',
        'double',
        'hold',
      ]);
    }

    // Key 1 configures `single`, `double` and `hold`; key 2 configures none.
    const [first, second] = cards();
    const active = (card: HTMLElement, trigger: string) =>
      within(card)
        .getAllByTestId('key-card-trigger')
        .find((pill) => pill.getAttribute('data-trigger') === trigger)
        ?.getAttribute('data-active');

    expect(active(first, 'single')).toBe('true');
    expect(active(first, 'double')).toBe('true');
    expect(active(first, 'hold')).toBe('true');

    expect(active(second, 'single')).toBe('false');
    expect(active(second, 'double')).toBe('false');
    expect(active(second, 'hold')).toBe('false');
  });

  it('keeps a responsive grid that never drops below two columns', async () => {
    await renderPage();

    // The grid publishes the layout contract its stylesheet consumes: four
    // columns when there is room, and a minimum track capped at a share of the
    // container so two of them always fit.
    const style = getComputedStyle(screen.getByTestId('key-grid'));
    expect(Number(style.getPropertyValue('--key-grid-columns'))).toBe(4);
    const minColumns = Number(style.getPropertyValue('--key-grid-min-columns'));
    expect(minColumns).toBeGreaterThanOrEqual(2);

    const share = /min\((\d+)%/.exec(style.getPropertyValue('--key-grid-min-track'));
    expect(share, 'minimum track is a share of the container').not.toBeNull();
    expect(Number(share?.[1]) * minColumns).toBeLessThanOrEqual(100);
  });
});

// -- mode switch -------------------------------------------------------------

describe('mode switch', () => {
  it('asks the core to switch mode and renders that mode’s cards', async () => {
    await renderPage();
    expect(cardLabels()).toEqual(DESKTOP_LABELS);

    await fireEvent.click(screen.getByRole('button', { name: 'Home Assistant' }));
    await flush();

    expect(invokeMock).toHaveBeenCalledWith('set_mode_cmd', { index: 1 });

    // The grid follows the core's event, not the click.
    currentSnapshot.modeIndex = 1;
    currentSnapshot.config.activeModeIndex = 1;
    emit('octopad://mode-changed', { mode_index: 1 });
    await flush();

    expect(cards()).toHaveLength(8);
    expect(cardLabels()).toEqual(HA_LABELS);
    expect(screen.queryByText('Terminal')).toBeNull();
    expect(
      screen.getByRole('button', { name: 'Home Assistant' }).getAttribute('aria-pressed')
    ).toBe('true');
    expect(screen.getByRole('button', { name: 'Desktop' }).getAttribute('aria-pressed')).toBe('false');
  });

  it('keeps showing the confirmed mode when the core rejects the switch', async () => {
    await renderPage();

    invokeMock.mockImplementation((command: string) => {
      commandLog.push(command);
      if (command === 'get_state_snapshot') {
        return Promise.resolve(structuredClone(currentSnapshot));
      }
      if (command === 'list_presets_cmd') return Promise.resolve([]);
      if (command === 'set_mode_cmd') return Promise.resolve(false);
      return Promise.resolve(true);
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Home Assistant' }));
    await flush();

    expect(cardLabels()).toEqual(DESKTOP_LABELS);
  });
});

// -- selection ---------------------------------------------------------------

describe('selection', () => {
  it('asks the core to select the clicked key and highlights what the core confirms', async () => {
    await renderPage();

    await fireEvent.click(screen.getByRole('button', { name: 'Key 3: Mail' }));
    await flush();

    expect(invokeMock).toHaveBeenCalledWith('select_button_cmd', { index: 2 });
    // Nothing is patched locally: the highlight only follows the core's event.
    expect(selectedCardKeys()).toEqual(['0']);

    emit('octopad://selection-changed', { index: 2 });
    await flush();

    expect(selectedCardKeys()).toEqual(['2']);
    const selected = screen.getByRole('button', { name: 'Key 3: Mail' });
    expect(selected.getAttribute('aria-pressed')).toBe('true');
    expect(selected.getAttribute('data-selected')).toBe('true');
    expect(selected.classList.contains('is-selected')).toBe(true);
    expect(
      screen.getByRole('button', { name: 'Key 1: Terminal' }).getAttribute('aria-pressed')
    ).toBe('false');
  });

  it('re-reads labels from the core after a button-changed event', async () => {
    await renderPage();

    currentSnapshot.config.modes[0].buttons[0].label = 'Terminal (renamed)';
    emit('octopad://button-changed', { index: 0 });
    await flush();

    expect(cardLabels()[0]).toBe('Terminal (renamed)');
  });
});

// -- sequential edit safety --------------------------------------------------

describe('command ordering', () => {
  it('awaits the selection command before running an edit', async () => {
    const gate = deferred<boolean>();
    invokeMock.mockImplementation((command: string) => {
      commandLog.push(command);
      if (command === 'get_state_snapshot') {
        return Promise.resolve(structuredClone(currentSnapshot));
      }
      if (command === 'list_presets_cmd') return Promise.resolve([]);
      if (command === 'select_button_cmd') return gate.promise;
      return Promise.resolve(true);
    });

    const view = await renderPage();

    fireEvent.click(screen.getByRole('button', { name: 'Key 5: Music' }));
    await flush();
    expect(commandLog).toContain('select_button_cmd');

    // An edit queued while the selection is still in flight must not run yet.
    const edits: string[] = [];
    const edited = editorOf(view).runEdit(async () => {
      edits.push('label');
      return true;
    });
    await flush();
    expect(edits).toEqual([]);

    gate.resolve(true);
    await expect(edited).resolves.toBe(true);
    expect(edits).toEqual(['label']);
    expect(commandLog).toEqual(['get_state_snapshot', 'select_button_cmd']);
  });

  it('keeps rapid clicks in order so a fast click never lands on a stale selection', async () => {
    const gate = deferred<boolean>();
    let selections = 0;
    invokeMock.mockImplementation((command: string) => {
      commandLog.push(command);
      if (command === 'get_state_snapshot') {
        return Promise.resolve(structuredClone(currentSnapshot));
      }
      if (command === 'list_presets_cmd') return Promise.resolve([]);
      if (command === 'select_button_cmd') {
        selections += 1;
        // The first click is still in flight when the second one is fired.
        return selections === 1 ? gate.promise : Promise.resolve(true);
      }
      return Promise.resolve(true);
    });

    await renderPage();

    fireEvent.click(screen.getByRole('button', { name: 'Key 4: Chat' }));
    fireEvent.click(screen.getByRole('button', { name: 'Key 7: Stream' }));
    await flush();

    // Only the first click reached the core; the second is queued behind it.
    const issuedWhileBusy = invokeMock.mock.calls
      .filter((call) => call[0] === 'select_button_cmd')
      .map((call) => (call[1] as { index: number }).index);
    expect(issuedWhileBusy).toEqual([3]);

    gate.resolve(true);
    await flush();

    const issued = invokeMock.mock.calls
      .filter((call) => call[0] === 'select_button_cmd')
      .map((call) => (call[1] as { index: number }).index);
    expect(issued).toEqual([3, 6]);
  });

  it('still runs later commands after a failed selection', async () => {
    invokeMock.mockImplementation((command: string) => {
      commandLog.push(command);
      if (command === 'get_state_snapshot') {
        return Promise.resolve(structuredClone(currentSnapshot));
      }
      if (command === 'list_presets_cmd') return Promise.resolve([]);
      if (command === 'select_button_cmd') return Promise.reject(new Error('ipc unavailable'));
      return Promise.resolve(true);
    });

    const view = await renderPage();

    await fireEvent.click(screen.getByRole('button', { name: 'Key 2: Browser' }));
    await flush();

    expect(selectedCardKeys()).toEqual(['0']);

    // The chain survives the rejection, so the next command still runs.
    await expect(editorOf(view).runEdit(async () => true)).resolves.toBe(true);
    expect(get(appState)?.selectedIndex).toBe(0);
  });
});