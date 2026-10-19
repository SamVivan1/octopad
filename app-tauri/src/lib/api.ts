/**
 * Typed bridge to the Tauri command surface.
 *
 * Every wrapper here is a one-line delegation to `invoke`, so the command
 * names, argument names and return types are pinned in one place and the rest
 * of the frontend never touches `@tauri-apps/api` directly.
 *
 * Rust command bodies stay thin and report failure by returning `false` or by
 * rejecting (`src-tauri/src/commands/device.rs`); these wrappers deliberately
 * do **not** swallow that — `runCommand` in `./state` turns it into visible
 * feedback.
 */

import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';

import type {
  Action,
  AppEventPayloads,
  AppSnapshot,
  ImportReport,
  TimingField,
  Trigger,
} from './types';

export type { UnlistenFn };

/**
 * Every event the core can emit, prefixed exactly as `src-tauri/src/lib.rs`
 * emits it: `octopad://<event-name>`.
 *
 * Declaration order is the listener registration order.
 */
export const OCTOPAD_EVENTS = {
  dirtyChanged: 'octopad://dirty-changed',
  configReplaced: 'octopad://config-replaced',
  buttonChanged: 'octopad://button-changed',
  timingChanged: 'octopad://timing-changed',
  modeChanged: 'octopad://mode-changed',
  selectionChanged: 'octopad://selection-changed',
  configSaved: 'octopad://config-saved',
  presetsChanged: 'octopad://presets-changed',
  serialStatusChanged: 'octopad://serial-status-changed',
  bleStatusChanged: 'octopad://ble-status-changed',
  deviceNameChanged: 'octopad://device-name-changed',
  deviceInfoChanged: 'octopad://device-info-changed',
  serialLog: 'octopad://serial-log',
  haStatusChanged: 'octopad://ha-status-changed',
  haEntitiesChanged: 'octopad://ha-entities-changed',
  appsChanged: 'octopad://apps-changed',
} as const satisfies Record<string, keyof AppEventPayloads>;

export type OctopadEventName = (typeof OCTOPAD_EVENTS)[keyof typeof OCTOPAD_EVENTS];

/** Tauri maps Rust snake_case command arguments to camelCase; all of these are single words. */
export type InvokeArgs = Record<string, unknown>;

/** Raw `invoke`, exposed only for tests and for commands added later. */
export function invoke<T>(command: string, args?: InvokeArgs): Promise<T> {
  return tauriInvoke<T>(command, args);
}

/** Subscribe to a core event; the handler receives the flat payload, not the Tauri envelope. */
export function listenAppEvent<E extends OctopadEventName>(
  event: E,
  handler: (payload: AppEventPayloads[E]) => void
): Promise<UnlistenFn> {
  return tauriListen<AppEventPayloads[E]>(event, (message) => handler(message.payload));
}

// -- snapshot / config lifecycle ---------------------------------------------

export function getStateSnapshot(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('get_state_snapshot');
}

export function saveConfig(): Promise<boolean> {
  return invoke<boolean>('save_config_cmd');
}

export function listPresets(): Promise<string[]> {
  return invoke<string[]>('list_presets_cmd');
}

export function importLegacyConfig(): Promise<ImportReport> {
  return invoke<ImportReport>('import_legacy_config');
}

// -- navigation --------------------------------------------------------------

/** `false` when the index is outside the configured modes. */
export function setMode(index: number): Promise<boolean> {
  return invoke<boolean>('set_mode_cmd', { index });
}

/** `false` when the index is outside the current mode's buttons. */
export function selectButton(index: number): Promise<boolean> {
  return invoke<boolean>('select_button_cmd', { index });
}

// -- button / action edits ---------------------------------------------------

export function setLabel(text: string): Promise<void> {
  return invoke<void>('set_label_cmd', { text });
}

/** `false` unless `trigger` is `single`, `double` or `hold`. */
export function setAction(trigger: Trigger, index: number, action: Action): Promise<boolean> {
  return invoke<boolean>('set_action_cmd', { trigger, index, action });
}

/** `false` for an unknown trigger. */
export function addAction(trigger: Trigger): Promise<boolean> {
  return invoke<boolean>('add_action_cmd', { trigger });
}

/** `false` for an unknown trigger. */
export function removeAction(trigger: Trigger, index: number): Promise<boolean> {
  return invoke<boolean>('remove_action_cmd', { trigger, index });
}

/** `false` for an unknown trigger. */
export function moveAction(trigger: Trigger, index: number, delta: number): Promise<boolean> {
  return invoke<boolean>('move_action_cmd', { trigger, index, delta });
}

/** `false` for any field other than the three camelCase timing keys. */
export function setTiming(field: TimingField, value: number): Promise<boolean> {
  return invoke<boolean>('set_timing_cmd', { field, value });
}

// -- device ------------------------------------------------------------------

/** `false` when the name is rejected or the transport write fails. */
export function renameDevice(name: string): Promise<boolean> {
  return invoke<boolean>('rename_device_cmd', { name });
}

/**
 * Quit for real: signal runtime shutdown and exit, exactly like the tray Quit
 * item. Closing the window is deliberately *not* this command — that only hides
 * the window and leaves the device connections running.
 */
export function quitApp(): Promise<void> {
  return invoke<void>('quit_app_cmd');
}

/** `true` only after a transport reports a successful write. */
export function sendDeviceCommand(command: string): Promise<boolean> {
  return invoke<boolean>('send_device_command_cmd', { command });
}

/**
 * Render an `ImportError` rejection.
 *
 * `ImportError` serializes as an externally tagged value, so it arrives either
 * as a bare variant name (`"NotFound"`) or as `{"Parse": "…"}`.
 */
export function importErrorMessage(error: unknown): string {
  if (typeof error === 'string') {
    return error;
  }
  if (typeof error === 'object' && error !== null) {
    const [variant, detail] = Object.entries(error as Record<string, unknown>)[0] ?? [];
    if (variant) {
      return detail === undefined || detail === null ? variant : `${variant}: ${String(detail)}`;
    }
  }
  return 'legacy import failed';
}