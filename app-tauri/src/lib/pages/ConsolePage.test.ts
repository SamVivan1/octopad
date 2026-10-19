// @vitest-environment jsdom
/**
 * Contract tests for the live Serial Console (Task 7).
 *
 * The console is a thin view over two shared things: the bounded `logEntries`
 * buffer that `initializeAppState()` feeds from `octopad://serial-log`, and
 * `send_device_command_cmd`, which only returns `true` once a transport
 * reported a successful write. The fake core below reproduces those two rules —
 * the manager logs `[TX] …` / `[TX-BLE] …` *after* a successful write and says
 * nothing when the write fails — so the page is only ever asserted against
 * behaviour the real `octopad-core` has.
 *
 * What is pinned here:
 *
 * - Log lines from the shared layer render with a timestamp and a colour class.
 * - Classification separates the manager's TX record, lines the pad itself
 *   produced, and errors.
 * - An executed action is reported honestly: `[ACTION] … [FAIL]` is an error
 *   line, `[OK]` and the `Executing action for …` line are not, and a failure in
 *   the middle of a chain neither hides nor reorders the actions around it.
 * - The quick commands are exactly `WHO`, `INFO`, `SAVE`, `HELP`, each sent
 *   verbatim.
 * - Enter sends what was typed; a **failed** write shows an error and keeps the
 *   text so it can be retried; a **successful** write clears the input.
 * - The console never invents a success line of its own: the only record of a
 *   hardware write is the manager's `[TX]` event.
 * - Clear empties the view, and the view stays bounded: at the cap nothing is
 *   dropped, beyond it the oldest 200 lines are trimmed from what is displayed.
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

import ConsolePage from './ConsolePage.svelte';
import { appState, initializeAppState, logEntries, resetAppState } from '../state';
import type { AppSnapshot, Button, Config } from '../types';

// -- fixtures ----------------------------------------------------------------

const PINOUT = [20, 9, 2, 1, 21, 10, 3, 0];

function makeButtons(): Button[] {
  return PINOUT.map((pin, index) => ({
    index,
    pin,
    label: `Key ${index + 1}`,
    single: { type: 'none' },
    double: { type: 'none' },
    hold: { type: 'none' },
  }));
}

function makeConfig(): Config {
  return {
    version: 1,
    schemaVersion: 1,
    device: 'octopad',
    debounceMs: 25,
    clickTimeoutMs: 250,
    holdTimeoutMs: 450,
    activeModeIndex: 0,
    modes: [
      { id: 'desktop', name: 'Desktop', ledState: false, buttons: makeButtons() },
      { id: 'ha', name: 'Home Assistant', ledState: false, buttons: [] },
    ],
  };
}

function makeSnapshot(overrides: Partial<AppSnapshot> = {}): AppSnapshot {
  return {
    config: makeConfig(),
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

let handlers = new Map<string, EventHandler>();
let overrides = new Map<string, () => unknown>();
let snapshot: AppSnapshot = makeSnapshot();

/** Every command line the core asked the pad to write, in order. */
let writes: string[] = [];
/** Every `[TX]`/`[TX-BLE]` line the manager emitted, in order. */
let txLines: string[] = [];

function emit(name: string, payload: unknown): void {
  handlers.get(name)?.({ payload });
}

function fakeCore(command: string, args: Record<string, unknown> = {}): unknown {
  const override = overrides.get(command);
  if (override) return override();

  switch (command) {
    case 'get_state_snapshot':
      return Promise.resolve(structuredClone(snapshot));
    case 'list_presets_cmd':
      return Promise.resolve([]);

    case 'send_device_command_cmd': {
      // `DeviceManager::send_command`: the command is trimmed, the write has to
      // succeed, and only then is the `[TX]` / `[TX-BLE]` record emitted.
      const text = String(args.command ?? '').trim();
      if (text.length === 0 || !snapshot.serial.connected) {
        return Promise.resolve(false);
      }
      writes.push(text);
      const prefix = snapshot.ble.connected && !snapshot.serial.connected ? '[TX-BLE]' : '[TX]';
      const line = `${prefix} ${text}`;
      txLines.push(line);
      emit('octopad://serial-log', { line });
      return Promise.resolve(true);
    }

    default:
      return Promise.resolve(true);
  }
}

// -- harness -----------------------------------------------------------------

async function flush(): Promise<void> {
  for (let tick = 0; tick < 5; tick += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function renderConsole(): Promise<void> {
  render(ConsolePage);
  await flush();
}

function emitLog(line: string): void {
  emit('octopad://serial-log', { line });
}

/** Attach a connected pad and let the shared store learn about it. */
function connectSerial(): void {
  snapshot = makeSnapshot({ serial: { connected: true, port: '/dev/ttyACM0' } });
  emit('octopad://serial-status-changed', { connected: true, port: '/dev/ttyACM0' });
}

function renderedLines(): HTMLElement[] {
  return screen.queryAllByTestId('log-line');
}

function lineTexts(): string[] {
  return renderedLines().map((node) => node.getAttribute('data-line') ?? '');
}

function kindsOf(lines: string[]): Array<string | null> {
  const byText = new Map(renderedLines().map((node) => [node.getAttribute('data-line'), node]));
  return lines.map((line) => byText.get(line)?.getAttribute('data-kind') ?? null);
}

function commandInput(): HTMLInputElement {
  return screen.getByTestId('console-input') as HTMLInputElement;
}

function quickButtons(): HTMLElement[] {
  return screen.getAllByTestId('quick-command');
}

function sends(): Array<Record<string, unknown>> {
  return invokeMock.mock.calls
    .filter((call) => call[0] === 'send_device_command_cmd')
    .map((call) => (call[1] ?? {}) as Record<string, unknown>);
}

/** The frozen clock the log timestamps are rendered from. */
const FIXED_TIME = new Date(2026, 9, 19, 13, 45, 7);
const FIXED_STAMP = '13:45:07';

beforeEach(async () => {
  handlers = new Map();
  overrides = new Map();
  writes = [];
  txLines = [];
  snapshot = makeSnapshot();
  invokeMock.mockReset();
  listenMock.mockReset();
  invokeMock.mockImplementation((command: string, args: Record<string, unknown> = {}) =>
    fakeCore(command, args)
  );
  listenMock.mockImplementation(async (name: string, handler: EventHandler) => {
    handlers.set(name, handler);
    return vi.fn();
  });
  // Freeze only `Date`, so the rendered timestamps are deterministic while
  // `flush()` keeps using real timers.
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(FIXED_TIME);
  resetAppState();
  await initializeAppState();
});

afterEach(() => {
  cleanup();
  resetAppState();
  vi.useRealTimers();
});

// -- the shared log view -----------------------------------------------------

describe('live log', () => {
  it('renders buffered lines with a timestamp and a colour class', async () => {
    await renderConsole();

    emitLog('HELLO:OCTOPAD:Desk Pad:v1.4.2');
    await flush();

    const lines = renderedLines();
    expect(lines).toHaveLength(1);

    const line = lines[0]!;
    expect(line.getAttribute('data-line')).toBe('HELLO:OCTOPAD:Desk Pad:v1.4.2');
    expect(line.className).toContain('console-log__line--');
    expect(within(line).getByTestId('log-time').textContent?.trim()).toBe(FIXED_STAMP);
    expect(within(line).getByTestId('log-text').textContent).toBe('HELLO:OCTOPAD:Desk Pad:v1.4.2');

    // The view is fed by the shared store, not by a page-local buffer.
    expect(get(logEntries).map((entry) => entry.line)).toEqual(['HELLO:OCTOPAD:Desk Pad:v1.4.2']);
  });

  it('classifies TX writes, pad output and errors', async () => {
    await renderConsole();

    const lines = [
      '[TX] WHO',
      '[TX-BLE] INFO',
      'HELLO:OCTOPAD:Desk Pad:v1.4.2',
      'INFO:name=Desk Pad;mode=desktop',
      '[DESKTOP] Button 2 -> SINGLE',
      'SAVED',
      '[ERROR] USB read failed',
      '[BLE FAIL] link dropped',
      '[SYSTEM] Console ready',
    ];
    for (const line of lines) emitLog(line);
    await flush();

    expect(kindsOf(lines)).toEqual([
      'tx',
      'tx',
      'device',
      'device',
      'device',
      'device',
      'error',
      'error',
      'system',
    ]);

    // Each class is a real class, not only an attribute.
    const byText = new Map(renderedLines().map((node) => [node.getAttribute('data-line'), node]));
    expect(byText.get('[TX] WHO')?.className).toContain('console-log__line--tx');
    expect(byText.get('HELLO:OCTOPAD:Desk Pad:v1.4.2')?.className).toContain(
      'console-log__line--device'
    );
    expect(byText.get('[ERROR] USB read failed')?.className).toContain(
      'console-log__line--error'
    );
  });

  it('keeps the log order the core emitted', async () => {
    await renderConsole();

    for (const line of ['first', 'second', 'third']) emitLog(line);
    await flush();

    expect(lineTexts()).toEqual(['first', 'second', 'third']);
  });
});

// -- action execution --------------------------------------------------------

describe('action execution', () => {
  // The exact strings `exec::chain` emits for one `shortcut` action on key 1.
  const EXECUTING = 'Executing action for Button 1 (single) -> Type: shortcut';
  const OK = '[ACTION] B1 (SINGLE): Shortcut sent [OK]';
  const FAIL = '[ACTION] B1 (SINGLE): Shortcut sent [FAIL]';

  it('renders a failed action as an error and a delivered one as a normal line', async () => {
    await renderConsole();

    const lines = [EXECUTING, OK, FAIL];
    for (const line of lines) emitLog(line);
    await flush();

    expect(kindsOf(lines)).toEqual(['system', 'system', 'error']);

    const byText = new Map(renderedLines().map((node) => [node.getAttribute('data-line'), node]));
    expect(byText.get(FAIL)?.className).toContain('console-log__line--error');
    // An action that worked is not a failure, and announcing the action is not
    // one either: only the outcome decides.
    expect(byText.get(OK)?.className).not.toContain('console-log__line--error');
    expect(byText.get(EXECUTING)?.className).not.toContain('console-log__line--error');
  });

  it('keeps a failing action in the middle of a chain visible and in order', async () => {
    await renderConsole();

    // A chain that failed on its second action. The third action still ran, which
    // is what the console has to show rather than a truncated sequence.
    const lines = [
      'Executing action for Button 2 (single) -> Type: shortcut',
      '[ACTION] B2 (SINGLE): Shortcut sent [OK]',
      '[ACTION] B2 (SINGLE): Shortcut sent [FAIL]',
      'Executing action for Button 2 (single) -> Type: text',
      '[ACTION] B2 (SINGLE): Text typed [OK]',
    ];
    for (const line of lines) emitLog(line);
    await flush();

    expect(lineTexts()).toEqual(lines);
    expect(kindsOf(lines)).toEqual(['system', 'system', 'error', 'system', 'system']);

    await fireEvent.click(screen.getByTestId('console-clear'));
    await flush();

    expect(renderedLines()).toHaveLength(0);
    expect(screen.getByTestId('console-empty')).toBeDefined();
  });
});

// -- quick commands ----------------------------------------------------------

describe('quick commands', () => {
  it('offers exactly WHO, INFO, SAVE and HELP', async () => {
    await renderConsole();

    expect(quickButtons().map((button) => button.textContent?.trim())).toEqual([
      'WHO',
      'INFO',
      'SAVE',
      'HELP',
    ]);
  });

  it('sends each quick command verbatim', async () => {
    connectSerial();
    await renderConsole();

    for (const button of quickButtons()) {
      await fireEvent.click(button);
      await flush();
    }

    expect(writes).toEqual(['WHO', 'INFO', 'SAVE', 'HELP']);
    expect(sends()).toEqual([
      { command: 'WHO' },
      { command: 'INFO' },
      { command: 'SAVE' },
      { command: 'HELP' },
    ]);
  });
});

// -- the command input -------------------------------------------------------

describe('command input', () => {
  beforeEach(() => {
    connectSerial();
  });

  it('sends the typed text on Enter', async () => {
    await renderConsole();

    await fireEvent.change(commandInput(), { target: { value: 'NAME:Desk Pad' } });
    await fireEvent.keyDown(commandInput(), { key: 'Enter' });
    await flush();

    expect(writes).toEqual(['NAME:Desk Pad']);
    expect(sends()).toEqual([{ command: 'NAME:Desk Pad' }]);
  });

  it('clears the input after a successful write', async () => {
    await renderConsole();

    await fireEvent.change(commandInput(), { target: { value: 'WHO' } });
    await fireEvent.keyDown(commandInput(), { key: 'Enter' });
    await flush();

    expect(commandInput().value).toBe('');
    expect(screen.queryByTestId('console-error')).toBeNull();
  });

  it('sends what the Send button is given', async () => {
    await renderConsole();

    await fireEvent.change(commandInput(), { target: { value: '  TIMING:25,250,450  ' } });
    await fireEvent.click(screen.getByTestId('console-send'));
    await flush();

    // Trimmed exactly like `DeviceManager::send_command` does.
    expect(writes).toEqual(['TIMING:25,250,450']);
    expect(commandInput().value).toBe('');
  });

  it('ignores an empty command', async () => {
    await renderConsole();

    await fireEvent.change(commandInput(), { target: { value: '   ' } });
    await fireEvent.keyDown(commandInput(), { key: 'Enter' });
    await flush();

    expect(sends()).toEqual([]);
    expect(writes).toEqual([]);
    expect(screen.queryByTestId('console-error')).toBeNull();
  });

  it('shows an error and keeps the text when the write fails', async () => {
    // Disconnected: `send_device_command_cmd` returns false and writes nothing.
    snapshot = makeSnapshot();
    emit('octopad://serial-status-changed', { connected: false, port: null });
    await renderConsole();

    await fireEvent.change(commandInput(), { target: { value: 'WHO' } });
    await fireEvent.keyDown(commandInput(), { key: 'Enter' });
    await flush();

    expect(sends()).toEqual([{ command: 'WHO' }]);
    expect(writes).toEqual([]);

    const error = screen.getByTestId('console-error');
    expect(error.getAttribute('role')).toBe('alert');
    expect(error.textContent?.trim()).not.toBe('');
    expect(error.textContent).toContain('WHO');

    // The text survives so the same command can be retried once connected.
    expect(commandInput().value).toBe('WHO');
  });

  it('reports an IPC rejection as a failure and keeps the text', async () => {
    await renderConsole();
    overrides.set('send_device_command_cmd', () => Promise.reject(new Error('ipc down')));

    await fireEvent.change(commandInput(), { target: { value: 'HELP' } });
    await fireEvent.click(screen.getByTestId('console-send'));
    await flush();

    expect(screen.getByTestId('console-error').textContent).toContain('ipc down');
    expect(commandInput().value).toBe('HELP');
  });

  it('never writes a success line of its own', async () => {
    await renderConsole();

    await fireEvent.change(commandInput(), { target: { value: 'INFO' } });
    await fireEvent.keyDown(commandInput(), { key: 'Enter' });
    await flush();

    // Exactly the one line the manager logged for the write, nothing else.
    expect(lineTexts()).toEqual(txLines);
    expect(lineTexts()).toEqual(['[TX] INFO']);

    // And a failed write logs nothing at all.
    snapshot = makeSnapshot();
    overrides.set('send_device_command_cmd', () => Promise.resolve(false));
    await fireEvent.change(commandInput(), { target: { value: 'SAVE' } });
    await fireEvent.keyDown(commandInput(), { key: 'Enter' });
    await flush();

    expect(lineTexts()).toEqual(['[TX] INFO']);
  });
});

// -- clearing ----------------------------------------------------------------

describe('clear', () => {
  it('empties the log view without touching the shared snapshot', async () => {
    await renderConsole();

    for (const line of ['one', 'two', 'three']) emitLog(line);
    await flush();
    expect(renderedLines()).toHaveLength(3);

    await fireEvent.click(screen.getByTestId('console-clear'));
    await flush();

    expect(renderedLines()).toHaveLength(0);
    expect(screen.getByTestId('console-empty')).toBeDefined();

    // Clearing the view is not a device command.
    expect(invokeMock.mock.calls.map((call) => call[0])).not.toContain('send_device_command_cmd');
  });

  it('keeps logging after a clear', async () => {
    await renderConsole();

    emitLog('before');
    await flush();
    await fireEvent.click(screen.getByTestId('console-clear'));
    await flush();

    emitLog('after');
    await flush();

    expect(lineTexts()).toEqual(['after']);
  });
});

// -- the bounded view --------------------------------------------------------

describe('bounded view', () => {
  it('shows every line up to the cap and trims the oldest 200 beyond it', async () => {
    await renderConsole();

    // Fill the view to its 1500-line maximum.
    for (let index = 0; index < 1500; index += 1) emitLog(`line ${index}`);
    await flush();

    expect(renderedLines()).toHaveLength(1500);
    expect(lineTexts()[0]).toBe('line 0');
    expect(lineTexts()[1499]).toBe('line 1499');

    // One line more: the view can never exceed its maximum, so the oldest 200
    // of what it holds are trimmed and the newest lines stay.
    emitLog('line 1500');
    await flush();

    const visible = lineTexts();
    expect(visible.length).toBeLessThanOrEqual(1500);
    // The shared store already dropped its own oldest line, so the window that
    // reached 1500 was `line 1 … line 1500`; trimming its oldest 200 leaves
    // `line 200 … line 1500`.
    expect(visible.length).toBe(1301);
    expect(visible[0]).toBe('line 200');
    expect(visible.at(-1)).toBe('line 1500');
    expect(visible).not.toContain('line 0');
    expect(visible).not.toContain('line 199');
  });

  it('keeps streaming past the trim point without emptying itself', async () => {
    await renderConsole();

    for (let index = 0; index < 1500; index += 1) emitLog(`line ${index}`);
    await flush();

    for (let index = 1500; index < 1600; index += 1) emitLog(`line ${index}`);
    await flush();

    const visible = lineTexts();
    expect(visible.length).toBeGreaterThan(0);
    expect(visible.length).toBeLessThanOrEqual(1500);
    expect(visible.at(-1)).toBe('line 1599');
    expect(visible).not.toContain('line 0');
  });
});

// -- shell contract ----------------------------------------------------------

describe('console page', () => {
  it('reports which transport the pad is on', async () => {
    await renderConsole();

    expect(screen.getByTestId('console-transport').textContent).toContain('Not connected');

    cleanup();
    connectSerial();
    await renderConsole();

    expect(screen.getByTestId('console-transport').textContent).toContain('Serial');
    expect(get(appState)).not.toBeNull();
  });
});