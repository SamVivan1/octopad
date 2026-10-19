// @vitest-environment jsdom
/**
 * Contract tests for the button action editor (Task 5).
 *
 * The editor is reached through `ButtonsPage`, which is deliberate: the page
 * owns the serialized `runEdit` queue, and the whole point of these tests is
 * that *every* mutation the action editor issues travels through that queue.
 * Rendering the page therefore tests the editor and its wiring at once.
 *
 * The mocked `invoke` is a small stand-in for `octopad-core`: it applies
 * `set_action` / `add_action` / `remove_action` / `move_action` with the same
 * semantics as `AppState` (including `TriggerActions::from_list`, where a
 * single-action slot collapses back to the legacy object shape), and emits the
 * same `button-changed` event the real core does. That lets the round-trip
 * assertions be real: the UI only ever sees config that came back through the
 * snapshot.
 *
 * What is pinned here:
 *
 * - Every action type's editable fields, including the `delay` range
 *   (0–600 s, 0.1 step) and the "Script" label for `bash_script`.
 * - **Non-destructive edits.** Changing a known field leaves every other key
 *   alone, so metadata this build does not understand survives a JSON
 *   round-trip. List order is likewise preserved.
 * - **Both on-disk trigger shapes.** A legacy object slot stays an object and a
 *   list slot stays a list; the editor never re-shapes either.
 * - Add / remove / reorder with the core's "always keep one action" rule.
 * - Failures surface and are never reported as successful edits.
 * - Nothing executes: no action is ever sent to the device in M2.
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

import ButtonsPage from '../pages/ButtonsPage.svelte';
import { appState, initializeAppState, lastError, resetAppState } from '../state';
import type { Action, AppSnapshot, Button, Mode, Trigger, TriggerSlot } from '../types';

// -- fixtures ----------------------------------------------------------------

const PINOUT = [20, 9, 2, 1, 21, 10, 3, 0];

function isAction(value: unknown): value is Action {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    typeof (value as Action).type === 'string'
  );
}

/** The key under edit: a legacy slot, a list slot, and a single-action slot. */
function makeEditedButton(): Button {
  return {
    index: 0,
    pin: PINOUT[0],
    label: 'Terminal',
    // Legacy single-object slot, carrying metadata this build does not know.
    single: { type: 'media', mediaKey: 'PLAY_PAUSE', mediaArgs: { repeat: 2 }, note: 'legacy' },
    // List slot, in order: an empty entry, a text action, a delay.
    double: [
      { type: 'none' },
      { type: 'text', text: 'hello', charset: 'utf-8' },
      { type: 'delay', seconds: 1.5 },
    ],
    // Single-action legacy slot — the "always keep one action" minimum.
    hold: { type: 'none' },
  };
}

function makeMode(id: string, name: string): Mode {
  return {
    id,
    name,
    ledState: false,
    buttons: [
      makeEditedButton(),
      ...Array.from({ length: 7 }, (_unused, offset) => {
        const index = offset + 1;
        return {
          index,
          pin: PINOUT[index],
          label: `Key ${index + 1}`,
          single: { type: 'none' },
          double: { type: 'none' },
          hold: { type: 'none' },
        } satisfies Button;
      }),
    ],
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
      modes: [makeMode('desktop', 'Desktop'), makeMode('ha', 'Home Assistant')],
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

/** Replace the edited key's single trigger, keeping everything else. */
function withSingleSlot(slot: TriggerSlot): void {
  currentSnapshot.config.modes[0].buttons[0].single = slot;
}

// -- fake core --------------------------------------------------------------

type EventHandler = (event: { payload: unknown }) => void;

type Overrides = Map<string, (args: Record<string, unknown>) => unknown>;

let handlers = new Map<string, EventHandler>();
let overrides: Overrides = new Map();
let commandLog: string[] = [];
let currentSnapshot: AppSnapshot = makeSnapshot();

/** The `actions_list` view of a trigger slot (`TriggerActions::as_slice`). */
function slotActions(slot: TriggerSlot): Action[] {
  if (Array.isArray(slot)) return slot.filter(isAction);
  return isAction(slot) ? [structuredClone(slot) as Action] : [];
}

/** Mirror of `TriggerActions::from_list`: one item collapses to the dict shape. */
function slotFromList(actions: Action[]): TriggerSlot {
  if (actions.length === 0) return [];
  return actions.length === 1 ? actions[0] : actions;
}

function editedButton(): Button {
  return currentSnapshot.config.modes[currentSnapshot.modeIndex].buttons.find(
    (button) => button.index === currentSnapshot.selectedIndex
  ) as Button;
}

function readTrigger(args: Record<string, unknown>, trigger: string): Button[Trigger] {
  return editedButton()[trigger as Trigger];
}

function writeTrigger(args: Record<string, unknown>, trigger: string, value: TriggerSlot): void {
  (editedButton() as unknown as Record<string, TriggerSlot>)[trigger] = value;
}

function emit(name: string, payload: unknown): void {
  handlers.get(name)?.({ payload });
}

/** What `AppState` does after a successful button mutation. */
function buttonChanged(): void {
  currentSnapshot.dirty = true;
  emit('octopad://dirty-changed', { dirty: true });
  emit('octopad://button-changed', { index: currentSnapshot.selectedIndex });
}

function fakeCore(command: string, args: Record<string, unknown> = {}): unknown {
  switch (command) {
    case 'get_state_snapshot':
      return Promise.resolve(structuredClone(currentSnapshot));
    case 'list_presets_cmd':
      return Promise.resolve([]);

    case 'select_button_cmd': {
      const index = Number(args.index);
      const buttons = currentSnapshot.config.modes[currentSnapshot.modeIndex].buttons;
      if (!Number.isInteger(index) || index < 0 || index >= buttons.length) {
        return Promise.resolve(false);
      }
      currentSnapshot.selectedIndex = index;
      emit('octopad://selection-changed', { index });
      return Promise.resolve(true);
    }

    case 'set_mode_cmd': {
      const index = Number(args.index);
      if (!Number.isInteger(index) || index < 0 || index >= currentSnapshot.config.modes.length) {
        return Promise.resolve(false);
      }
      currentSnapshot.modeIndex = index;
      currentSnapshot.config.activeModeIndex = index;
      emit('octopad://mode-changed', { mode_index: index });
      return Promise.resolve(true);
    }

    case 'set_label_cmd':
      editedButton().label = String(args.text ?? '');
      buttonChanged();
      return Promise.resolve(undefined);

    case 'set_action_cmd': {
      const trigger = String(args.trigger);
      const index = Number(args.index);
      const actions = slotActions(readTrigger(args, trigger));
      const target = index < 0 ? index + actions.length : index;
      if (target >= 0 && target < actions.length) {
        actions[target] = structuredClone(args.action) as Action;
      } else {
        actions.push(structuredClone(args.action) as Action);
      }
      writeTrigger(args, trigger, slotFromList(actions));
      buttonChanged();
      return Promise.resolve(true);
    }

    case 'add_action_cmd': {
      const trigger = String(args.trigger);
      const actions = slotActions(readTrigger(args, trigger));
      actions.push({ type: 'none' });
      writeTrigger(args, trigger, slotFromList(actions));
      buttonChanged();
      return Promise.resolve(true);
    }

    case 'remove_action_cmd': {
      const trigger = String(args.trigger);
      const index = Number(args.index);
      const actions = slotActions(readTrigger(args, trigger));
      const target = index < 0 ? index + actions.length : index;
      if (target >= 0 && target < actions.length) actions.splice(target, 1);
      if (actions.length === 0) actions.push({ type: 'none' });
      writeTrigger(args, trigger, slotFromList(actions));
      buttonChanged();
      return Promise.resolve(true);
    }

    case 'move_action_cmd': {
      const trigger = String(args.trigger);
      const index = Number(args.index);
      const delta = Number(args.delta);
      const actions = slotActions(readTrigger(args, trigger));
      const target = index + delta;
      if (index < 0 || index >= actions.length || target < 0 || target >= actions.length) {
        return Promise.resolve(true);
      }
      const moved = actions[index];
      actions[index] = actions[target];
      actions[target] = moved;
      writeTrigger(args, trigger, slotFromList(actions));
      buttonChanged();
      return Promise.resolve(true);
    }

    default:
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

let disposers: Array<() => void> = [];

async function flush(): Promise<void> {
  for (let tick = 0; tick < 6; tick += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function renderEditor(): Promise<void> {
  disposers.push(await initializeAppState());
  render(ButtonsPage);
  await flush();
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function triggerList(trigger: Trigger): HTMLElement {
  const element = document.querySelector(
    `[data-testid="trigger-list"][data-trigger="${trigger}"]`
  );
  expect(element, `no list rendered for trigger ${trigger}`).not.toBeNull();
  return element as HTMLElement;
}

function actionCards(trigger: Trigger): HTMLElement[] {
  return within(triggerList(trigger)).queryAllByTestId('action-card');
}

function actionTypes(trigger: Trigger): string[] {
  return actionCards(trigger).map((card) => card.getAttribute('data-type') ?? '');
}

function cardControl(
  trigger: Trigger,
  index: number,
  testid: string
): HTMLElement {
  const cards = actionCards(trigger);
  expect(cards.length, `no action card ${index} in ${trigger}`).toBeGreaterThan(index);
  const control = within(cards[index]).queryByTestId(testid);
  expect(control, `no "${testid}" on action ${index} of ${trigger}`).not.toBeNull();
  return control as HTMLElement;
}

function field(trigger: Trigger, index: number, name: string): HTMLElement {
  const cards = actionCards(trigger);
  const element = cards[index].querySelector(`[data-field="${name}"]`);
  expect(element, `no "${name}" field on action ${index} of ${trigger}`).not.toBeNull();
  return element as HTMLElement;
}

async function setField(
  trigger: Trigger,
  index: number,
  name: string,
  value: string
): Promise<void> {
  await fireEvent.change(field(trigger, index, name), { target: { value } });
}

function setActionCalls(command: string): Array<Record<string, unknown>> {
  return invokeMock.mock.calls
    .filter((call) => call[0] === command)
    .map((call) => (call[1] ?? {}) as Record<string, unknown>);
}

async function lastSetAction(): Promise<Record<string, unknown>> {
  const calls = setActionCalls('set_action_cmd');
  expect(calls.length, 'no set_action_cmd was issued').toBeGreaterThan(0);
  return calls[calls.length - 1];
}

beforeEach(() => {
  handlers = new Map();
  overrides = new Map();
  commandLog = [];
  currentSnapshot = makeSnapshot();
  invokeMock.mockReset();
  listenMock.mockReset();
  listenMock.mockImplementation(async (name: string, handler: EventHandler) => {
    const unlisten = vi.fn();
    handlers.set(name, handler);
    return unlisten;
  });
  invokeMock.mockImplementation((command: string, args: Record<string, unknown> = {}) => {
    commandLog.push(command);
    const override = overrides.get(command);
    return override ? override(args) : fakeCore(command, args);
  });
  resetAppState();
});

afterEach(() => {
  cleanup();
  for (const dispose of disposers) dispose();
  disposers = [];
  resetAppState();
});

// -- label editor ------------------------------------------------------------

describe('label editor', () => {
  it('sends the new label through the page command queue and shows what the core stored', async () => {
    await renderEditor();

    const input = screen.getByTestId('action-label') as HTMLInputElement;
    expect(input.value).toBe('Terminal');

    await fireEvent.change(input, { target: { value: 'Shell' } });
    await flush();

    expect(setActionCalls('set_action_cmd')).toEqual([]);
    expect(invokeMock).toHaveBeenCalledWith('set_label_cmd', { text: 'Shell' });
    expect(currentSnapshot.config.modes[0].buttons[0].label).toBe('Shell');
    // The core's event re-reads the snapshot; nothing is patched locally.
    expect((screen.getByTestId('action-label') as HTMLInputElement).value).toBe('Shell');
    expect(screen.getByRole('button', { name: 'Key 1: Shell' })).toBeTruthy();
  });

  it('surfaces a rejected label command and keeps the stored label', async () => {
    await renderEditor();
    rejectCommand('set_label_cmd');

    await fireEvent.change(screen.getByTestId('action-label'), { target: { value: 'Shell' } });
    await flush();

    expect(get(lastError)).toMatch(/label/i);
    expect(screen.getByTestId('action-error').textContent).toMatch(/label/i);
    expect(currentSnapshot.config.modes[0].buttons[0].label).toBe('Terminal');
  });
});

// -- per-type fields ---------------------------------------------------------

describe('action types without fields', () => {
  it('shows no field editors for none and mode_toggle', async () => {
    withSingleSlot([{ type: 'none' }, { type: 'mode_toggle' }]);
    await renderEditor();

    expect(actionTypes('single')).toEqual(['none', 'mode_toggle']);
    for (const index of [0, 1]) {
      expect(actionCards('single')[index].querySelectorAll('[data-field]')).toHaveLength(0);
    }
    expect(actionCards('single')[0].textContent).toMatch(/empty/i);
  });

  it('offers every action type, labelling bash_script as Script', async () => {
    await renderEditor();

    const select = cardControl('single', 0, 'action-type') as HTMLSelectElement;
    const options = Array.from(select.options).map((option) => [
      option.value,
      option.textContent?.trim(),
    ]);

    expect(options).toEqual([
      ['none', 'None'],
      ['shortcut', 'Shortcut'],
      ['text', 'Text'],
      ['media', 'Media'],
      ['audio_output', 'Audio Output'],
      ['launch_app', 'App'],
      ['system_action', 'System'],
      ['bash_script', 'Script'],
      ['home_assistant', 'Home Assistant'],
      ['ha_webhook', 'Webhook'],
      ['delay', 'Delay'],
      ['mode_toggle', 'Mode'],
    ]);
  });
});

describe('action field editors', () => {
  it('edits every field of the shortcut action: modifiers and key', async () => {
    withSingleSlot([{ type: 'shortcut', modifiers: ['ctrl'], key: 'A', repeats: 3 }]);
    await renderEditor();

    const shift = actionCards('single')[0].querySelector(
      '[data-modifier="shift"]'
    ) as HTMLInputElement;
    expect(shift.checked).toBe(false);
    await fireEvent.click(shift);
    await flush();

    expect(await lastSetAction()).toEqual({
      trigger: 'single',
      index: 0,
      action: { type: 'shortcut', modifiers: ['ctrl', 'shift'], key: 'A', repeats: 3 },
    });

    await setField('single', 0, 'key', 'F5');
    await flush();

    expect(await lastSetAction()).toEqual({
      trigger: 'single',
      index: 0,
      action: { type: 'shortcut', modifiers: ['ctrl', 'shift'], key: 'F5', repeats: 3 },
    });
  });

  it('edits the shortcut key even when the stored key is not in the list', async () => {
    withSingleSlot([{ type: 'shortcut', modifiers: [], key: 'XF86LaunchA' }]);
    await renderEditor();

    const select = field('single', 0, 'key') as HTMLSelectElement;
    expect(select.value).toBe('XF86LaunchA');

    await setField('single', 0, 'key', 'SPACE');
    await flush();

    expect((await lastSetAction()).action).toEqual({
      type: 'shortcut',
      modifiers: [],
      key: 'SPACE',
    });
  });

  it('edits the multi-line text of a text action', async () => {
    withSingleSlot([{ type: 'text', text: 'hi' }]);
    await renderEditor();

    await setField('single', 0, 'text', 'first line\nsecond line');
    await flush();

    expect(await lastSetAction()).toEqual({
      trigger: 'single',
      index: 0,
      action: { type: 'text', text: 'first line\nsecond line' },
    });
  });

  it('edits the media key', async () => {
    withSingleSlot([{ type: 'media', mediaKey: 'VOL_UP' }]);
    await renderEditor();

    await setField('single', 0, 'mediaKey', 'PLAY_PAUSE');
    await flush();

    expect((await lastSetAction()).action).toEqual({ type: 'media', mediaKey: 'PLAY_PAUSE' });
  });

  it('edits the ordered audio output identifiers', async () => {
    withSingleSlot([
      {
        type: 'audio_output',
        sinks: [
          { name: 'sink-a', description: 'Speakers', icon: 'audio-speakers-symbolic' },
          { name: 'sink-b', description: 'Headset' },
        ],
      },
    ]);
    await renderEditor();

    const rows = within(actionCards('single')[0]).getAllByTestId('sink-row');
    expect(rows).toHaveLength(2);
    expect((rows[0].querySelector('[data-field="name"]') as HTMLInputElement).value).toBe('sink-a');

    await fireEvent.change(rows[0].querySelector('[data-field="name"]') as HTMLInputElement, {
      target: { value: 'sink-a-renamed' },
    });
    await flush();

    expect((await lastSetAction()).action).toEqual({
      type: 'audio_output',
      sinks: [
        { name: 'sink-a-renamed', description: 'Speakers', icon: 'audio-speakers-symbolic' },
        { name: 'sink-b', description: 'Headset' },
      ],
    });

    const after = within(actionCards('single')[0]).getAllByTestId('sink-row');
    await fireEvent.click(within(after[0]).getByTestId('sink-remove'));
    await flush();

    expect((await lastSetAction()).action).toEqual({
      type: 'audio_output',
      sinks: [{ name: 'sink-b', description: 'Headset' }],
    });
  });

  it('adds an audio output sink', async () => {
    withSingleSlot([{ type: 'audio_output', sinks: [] }]);
    await renderEditor();

    await fireEvent.click(within(actionCards('single')[0]).getByTestId('sink-add'));
    await flush();

    expect((await lastSetAction()).action).toEqual({
      type: 'audio_output',
      sinks: [{ name: '', description: '' }],
    });
  });

  it('edits the launch app name and path', async () => {
    withSingleSlot([{ type: 'launch_app', appName: 'Terminal', desktopId: 'org.gnome.Terminal' }]);
    await renderEditor();

    await setField('single', 0, 'appName', 'Console');
    await flush();
    expect((await lastSetAction()).action).toEqual({
      type: 'launch_app',
      appName: 'Console',
      desktopId: 'org.gnome.Terminal',
    });

    await setField('single', 0, 'desktopId', 'utilities-terminal');
    await flush();
    expect((await lastSetAction()).action).toEqual({
      type: 'launch_app',
      appName: 'Console',
      desktopId: 'utilities-terminal',
    });
  });

  it('edits the system action identifier and keeps an unknown preset selectable', async () => {
    withSingleSlot([{ type: 'system_action', presetId: 'custom_thing', presetName: 'Custom' }]);
    await renderEditor();

    const select = field('single', 0, 'presetId') as HTMLSelectElement;
    expect(select.value).toBe('custom_thing');

    await setField('single', 0, 'presetId', 'lock_screen');
    await flush();
    // Only the edited field changes; `presetName` is untouched.
    expect((await lastSetAction()).action).toEqual({
      type: 'system_action',
      presetId: 'lock_screen',
      presetName: 'Custom',
    });
  });

  it('edits the script text of a bash_script action', async () => {
    withSingleSlot([{ type: 'bash_script', script: 'echo hi' }]);
    await renderEditor();

    await setField('single', 0, 'script', 'notify-send "OctoPad" "hi"');
    await flush();

    expect((await lastSetAction()).action).toEqual({
      type: 'bash_script',
      script: 'notify-send "OctoPad" "hi"',
    });
  });

  it('edits the Home Assistant entity, domain, service and data', async () => {
    withSingleSlot([
      {
        type: 'home_assistant',
        entityId: 'light.hall',
        domain: 'light',
        service: 'toggle',
        data: { brightness: 80 },
        friendlyName: 'Hall',
      },
    ]);
    await renderEditor();

    await setField('single', 0, 'entityId', 'light.desk');
    await flush();
    expect((await lastSetAction()).action).toEqual({
      type: 'home_assistant',
      entityId: 'light.desk',
      domain: 'light',
      service: 'toggle',
      data: { brightness: 80 },
      friendlyName: 'Hall',
    });

    await setField('single', 0, 'domain', 'switch');
    await flush();
    expect((await lastSetAction()).action).toMatchObject({ domain: 'switch' });

    await setField('single', 0, 'service', 'turn_on');
    await flush();
    expect((await lastSetAction()).action).toMatchObject({ service: 'turn_on' });

    await setField('single', 0, 'data', '{"brightness": 120}');
    await flush();
    expect((await lastSetAction()).action).toEqual({
      type: 'home_assistant',
      entityId: 'light.desk',
      domain: 'switch',
      service: 'turn_on',
      data: { brightness: 120 },
      friendlyName: 'Hall',
    });
  });

  it('refuses to send malformed Home Assistant data', async () => {
    withSingleSlot([{ type: 'home_assistant', entityId: 'light.hall', data: {} }]);
    await renderEditor();

    await setField('single', 0, 'data', '{not json');
    await flush();

    expect(setActionCalls('set_action_cmd')).toEqual([]);
    const error = actionCards('single')[0].querySelector('[data-testid="action-field-error"]');
    expect(error?.textContent).toMatch(/json/i);
    expect(currentSnapshot.config.modes[0].buttons[0].single).toEqual([
      { type: 'home_assistant', entityId: 'light.hall', data: {} },
    ]);
  });

  it('rejects malformed Home Assistant service data without changing the action', async () => {
    withSingleSlot([{ type: 'home_assistant', entityId: 'light.hall', data: {} }]);
    await renderEditor();

    // Parseable JSON that is not a JSON object is still malformed for
    // `service.data`: the core sends it verbatim as the service call payload.
    for (const raw of ['[1, 2]', '"str"', '42', 'null']) {
      await setField('single', 0, 'data', raw);
      await flush();

      expect(setActionCalls('set_action_cmd'), `accepted ${raw}`).toEqual([]);
      const error = actionCards('single')[0].querySelector('[data-testid="action-field-error"]');
      expect(error?.textContent, `no error for ${raw}`).toMatch(/json object/i);
      expect(currentSnapshot.config.modes[0].buttons[0].single).toEqual([
        { type: 'home_assistant', entityId: 'light.hall', data: {} },
      ]);
    }
  });

  it('edits the webhook URL', async () => {
    withSingleSlot([{ type: 'ha_webhook', url: '' }]);
    await renderEditor();

    await setField('single', 0, 'url', 'http://ha.local:8123/api/webhook/pad');
    await flush();

    expect((await lastSetAction()).action).toEqual({
      type: 'ha_webhook',
      url: 'http://ha.local:8123/api/webhook/pad',
    });
  });

  it('constrains the delay to 0-600 seconds in steps of 0.1', async () => {
    withSingleSlot([{ type: 'delay', seconds: 0.5 }]);
    await renderEditor();

    const input = field('single', 0, 'seconds') as HTMLInputElement;
    expect(input.getAttribute('type')).toBe('number');
    expect(input.getAttribute('min')).toBe('0');
    expect(input.getAttribute('max')).toBe('600');
    expect(input.getAttribute('step')).toBe('0.1');

    await setField('single', 0, 'seconds', '2.5');
    await flush();

    expect((await lastSetAction()).action).toEqual({ type: 'delay', seconds: 2.5 });
    expect((field('single', 0, 'seconds') as HTMLInputElement).value).toBe('2.5');

    // A number input accepts more precision than the 0.1 step; it is rounded.
    await setField('single', 0, 'seconds', '3.14159');
    await flush();

    expect((await lastSetAction()).action).toEqual({ type: 'delay', seconds: 3.1 });
  });
});

// -- non-destructive edits ---------------------------------------------------

describe('non-destructive editing', () => {
  it('keeps unknown metadata when a known field is edited', async () => {
    await renderEditor();

    await setField('single', 0, 'mediaKey', 'MUTE');
    await flush();

    const call = await lastSetAction();
    expect(call.action).toEqual({
      type: 'media',
      mediaKey: 'MUTE',
      mediaArgs: { repeat: 2 },
      note: 'legacy',
    });
    // The stored action keeps every key it started with.
    expect(currentSnapshot.config.modes[0].buttons[0].single).toEqual({
      type: 'media',
      mediaKey: 'MUTE',
      mediaArgs: { repeat: 2 },
      note: 'legacy',
    });
  });

  it('keeps `type` as the first key of the stored action', async () => {
    await renderEditor();

    await setField('single', 0, 'mediaKey', 'NEXT_TRACK');
    await flush();

    const stored = currentSnapshot.config.modes[0].buttons[0].single as Action;
    expect(Object.keys(stored)[0]).toBe('type');
  });

  it('keeps metadata that no editor displays when the type changes', async () => {
    await renderEditor();

    await fireEvent.change(cardControl('single', 0, 'action-type'), {
      target: { value: 'text' },
    });
    await flush();

    expect((await lastSetAction()).action).toEqual({
      type: 'text',
      text: 'Hello World',
      mediaArgs: { repeat: 2 },
      note: 'legacy',
    });
  });

  it('preserves list order and untouched siblings when one action is edited', async () => {
    await renderEditor();

    await setField('double', 1, 'text', 'goodbye');
    await flush();

    const call = await lastSetAction();
    expect(call.trigger).toBe('double');
    expect(call.index).toBe(1);
    expect(call.action).toEqual({ type: 'text', text: 'goodbye', charset: 'utf-8' });
    // Only one command: no re-adding, no reordering, no sibling rewrites.
    expect(setActionCalls('set_action_cmd')).toHaveLength(1);
    expect(currentSnapshot.config.modes[0].buttons[0].double).toEqual([
      { type: 'none' },
      { type: 'text', text: 'goodbye', charset: 'utf-8' },
      { type: 'delay', seconds: 1.5 },
    ]);
  });
});

// -- add / remove / reorder --------------------------------------------------

describe('action list structure', () => {
  it('adds an action to a trigger', async () => {
    await renderEditor();

    await fireEvent.click(within(triggerList('double')).getByTestId('action-add'));
    await flush();

    expect(invokeMock).toHaveBeenCalledWith('add_action_cmd', { trigger: 'double' });
    expect(actionCards('double')).toHaveLength(4);
    expect(actionTypes('double')).toEqual(['none', 'text', 'delay', 'none']);
    expect(currentSnapshot.config.modes[0].buttons[0].double).toHaveLength(4);
  });

  it('removes the chosen action and keeps the others in order', async () => {
    await renderEditor();

    await fireEvent.click(within(actionCards('double')[1]).getByTestId('action-remove'));
    await flush();

    expect(invokeMock).toHaveBeenCalledWith('remove_action_cmd', { trigger: 'double', index: 1 });
    expect(actionTypes('double')).toEqual(['none', 'delay']);
  });

  it('moves an action up and down without touching the rest', async () => {
    await renderEditor();
    expect(actionTypes('double')).toEqual(['none', 'text', 'delay']);

    // Move the text action up: only positions 0 and 1 swap.
    await fireEvent.click(within(actionCards('double')[1]).getByTestId('action-move-up'));
    await flush();

    expect(invokeMock).toHaveBeenCalledWith('move_action_cmd', {
      trigger: 'double',
      index: 1,
      delta: -1,
    });
    expect(actionTypes('double')).toEqual(['text', 'none', 'delay']);
    expect(currentSnapshot.config.modes[0].buttons[0].double).toEqual([
      { type: 'text', text: 'hello', charset: 'utf-8' },
      { type: 'none' },
      { type: 'delay', seconds: 1.5 },
    ]);

    // Move it back down by clicking the entry that now sits at position 1.
    await fireEvent.click(within(actionCards('double')[1]).getByTestId('action-move-down'));
    await flush();

    expect(invokeMock).toHaveBeenCalledWith('move_action_cmd', {
      trigger: 'double',
      index: 1,
      delta: 1,
    });
    expect(actionTypes('double')).toEqual(['text', 'delay', 'none']);

    // And put the list back the way it was, one command at a time.
    await fireEvent.click(within(actionCards('double')[1]).getByTestId('action-move-down'));
    await flush();
    expect(actionTypes('double')).toEqual(['text', 'none', 'delay']);

    await fireEvent.click(within(actionCards('double')[0]).getByTestId('action-move-down'));
    await flush();
    expect(actionTypes('double')).toEqual(['none', 'text', 'delay']);
    expect(currentSnapshot.config.modes[0].buttons[0].double).toEqual([
      { type: 'none' },
      { type: 'text', text: 'hello', charset: 'utf-8' },
      { type: 'delay', seconds: 1.5 },
    ]);
  });

  it('disables the outer move buttons at the ends of the list', async () => {
    await renderEditor();

    expect(
      (within(actionCards('double')[0]).getByTestId('action-move-up') as HTMLButtonElement).disabled
    ).toBe(true);
    expect(
      (within(actionCards('double')[2]).getByTestId('action-move-down') as HTMLButtonElement)
        .disabled
    ).toBe(true);
  });

  it('keeps at least one action per trigger', async () => {
    await renderEditor();

    const remove = within(actionCards('hold')[0]).getByTestId('action-remove') as HTMLButtonElement;
    expect(remove.disabled).toBe(true);

    await fireEvent.click(remove);
    await flush();

    expect(setActionCalls('remove_action_cmd')).toEqual([]);
    expect(actionCards('hold')).toHaveLength(1);
    expect(currentSnapshot.config.modes[0].buttons[0].hold).toEqual({ type: 'none' });
  });
});

// -- trigger slot shapes -----------------------------------------------------

describe('trigger slot shapes', () => {
  it('round-trips a legacy single-object slot without changing its shape', async () => {
    await renderEditor();

    await setField('single', 0, 'mediaKey', 'PREV_TRACK');
    await flush();

    const slot = currentSnapshot.config.modes[0].buttons[0].single;
    expect(Array.isArray(slot)).toBe(false);
    expect(slot).toEqual({
      type: 'media',
      mediaKey: 'PREV_TRACK',
      mediaArgs: { repeat: 2 },
      note: 'legacy',
    });
    expect(actionCards('single')).toHaveLength(1);
  });

  it('round-trips a list slot without changing its shape or order', async () => {
    await renderEditor();

    await fireEvent.change(cardControl('double', 2, 'action-type'), {
      target: { value: 'ha_webhook' },
    });
    await flush();

    expect((await lastSetAction()).index).toBe(2);
    const slot = currentSnapshot.config.modes[0].buttons[0].double;
    expect(Array.isArray(slot)).toBe(true);
    expect(slot).toEqual([
      { type: 'none' },
      { type: 'text', text: 'hello', charset: 'utf-8' },
      { type: 'ha_webhook', url: '' },
    ]);
  });

  it('shows an unrecognized slot without rewriting it', async () => {
    withSingleSlot('a legacy scalar');
    await renderEditor();

    expect(actionCards('single')).toHaveLength(0);
    expect(triggerList('single').textContent).toMatch(/unrecognized|unsupported/i);
    expect(commandLog).not.toContain('set_action_cmd');
    expect(currentSnapshot.config.modes[0].buttons[0].single).toBe('a legacy scalar');
  });
});

// -- failures ----------------------------------------------------------------

describe('command failures', () => {
  it('surfaces a rejected field edit and never reports it as applied', async () => {
    await renderEditor();
    const before = structuredClone(currentSnapshot.config.modes[0].buttons[0].single);
    rejectCommand('set_action_cmd', 'error');

    await setField('single', 0, 'mediaKey', 'MUTE');
    await flush();

    expect(get(lastError)).toMatch(/failed/i);
    expect(screen.getByTestId('action-error').textContent).toMatch(/failed/i);
    expect(currentSnapshot.config.modes[0].buttons[0].single).toEqual(before);
    expect(get(appState)?.dirty).toBe(false);
  });

  it('surfaces a rejected remove and keeps the action in place', async () => {
    await renderEditor();
    rejectCommand('remove_action_cmd');

    await fireEvent.click(within(actionCards('double')[1]).getByTestId('action-remove'));
    await flush();

    expect(get(lastError)).toMatch(/rejected/i);
    expect(actionTypes('double')).toEqual(['none', 'text', 'delay']);
  });

  it('clears the error banner once a later command succeeds', async () => {
    await renderEditor();
    rejectCommand('remove_action_cmd');

    await fireEvent.click(within(actionCards('double')[1]).getByTestId('action-remove'));
    await flush();
    expect(screen.getByTestId('action-error')).toBeTruthy();

    overrides.delete('remove_action_cmd');
    await fireEvent.click(within(actionCards('double')[1]).getByTestId('action-remove'));
    await flush();

    expect(screen.queryByTestId('action-error')).toBeNull();
    expect(get(lastError)).toBeNull();
  });
});

// -- M2 scope ----------------------------------------------------------------

describe('M2 scope', () => {
  it('never executes or tests an action', async () => {
    await renderEditor();

    await setField('single', 0, 'mediaKey', 'MUTE');
    await fireEvent.click(within(triggerList('hold')).getByTestId('action-add'));
    await flush();

    const commands = invokeMock.mock.calls.map((call) => call[0]);
    expect(commands).not.toContain('send_device_command_cmd');
    expect(commands.filter((command) => /test|execute/i.test(String(command)))).toEqual([]);
    expect(within(actionCards('single')[0]).queryByTestId('action-test')).toBeNull();
  });
});

// -- command ordering --------------------------------------------------------

describe('command ordering', () => {
  it('queues an edit behind an in-flight key selection', async () => {
    const gate = deferred<boolean>();
    overrides.set('select_button_cmd', () => gate.promise);

    await renderEditor();

    fireEvent.click(screen.getByRole('button', { name: 'Key 5: Key 5' }));
    fireEvent.change(screen.getByTestId('action-label'), { target: { value: 'Racing' } });
    await flush();

    // The edit waits: it must not reach a selection the core has not confirmed.
    expect(commandLog).toContain('select_button_cmd');
    expect(commandLog).not.toContain('set_label_cmd');

    gate.resolve(true);
    await flush();

    expect(commandLog).toContain('set_label_cmd');
  });

  it('issues no edit at all for a trigger the core does not know', async () => {
    await renderEditor();

    // `add_action_cmd` rejects a trigger outside single/double/hold; the UI only
    // ever sends those three, so every mutation stays a valid command.
    const triggers = Array.from(document.querySelectorAll('[data-testid="trigger-list"]')).map(
      (list) => list.getAttribute('data-trigger')
    );
    expect(triggers).toEqual(['single', 'double', 'hold']);
  });
});