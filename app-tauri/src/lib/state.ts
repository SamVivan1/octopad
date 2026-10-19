/**
 * The single source of frontend truth: one snapshot store fed by the core's
 * event stream, plus a bounded in-memory log list.
 *
 * Rules this module exists to enforce (M2 Review Focus):
 *
 * - **Listeners before the snapshot.** `initializeAppState()` waits for every
 *   `listen` registration to resolve *before* it requests
 *   `get_state_snapshot`, so nothing emitted between startup and the first
 *   render is lost.
 * - **The core owns the config.** Nothing here edits a page-local copy of the
 *   config: events that carry no config data (`button-changed`,
 *   `config-replaced`) trigger a snapshot re-read, and everything else patches
 *   the single store in place.
 * - **Failures are visible.** `runCommand()` never turns a `false` return or a
 *   rejection into a silent no-op; it records a message in `lastError` and
 *   reports `false` to the caller.
 * - **Unknown payload fields are ignored.** Handlers read only the keys they
 *   know and type-check them, so a new core field cannot corrupt the store.
 */

import { writable } from 'svelte/store';
import type { Readable, Writable } from 'svelte/store';

import * as api from './api';
import { resetEditQueue } from './editQueue';
import type { AppEventPayloads, AppSnapshot, TimingField } from './types';

/** Newest log lines kept in memory; older lines are dropped on append. */
export const LOG_LIMIT = 1500;

/** One console line, with the moment it reached the webview. */
export interface LogEntry {
  id: number;
  line: string;
  receivedAt: number;
}

/** The whole read model, or `null` before the first snapshot is applied. */
export const appState: Writable<AppSnapshot | null> = writable<AppSnapshot | null>(null);

/** Bounded ring of serial-console / device lines, oldest first. */
export const logEntries: Writable<LogEntry[]> = writable<LogEntry[]>([]);

/** User preset slugs (`list_presets_cmd`); M3 turns these into the gallery. */
export const presets: Writable<string[]> = writable<string[]>([]);

/** Last command/event failure, for the UI to surface. `null` means "no failure". */
export const lastError: Writable<string | null> = writable<string | null>(null);

// -- payload readers ---------------------------------------------------------

type Payload = Record<string, unknown> | null;

function asPayload(value: unknown): Payload {
  if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function readBoolean(payload: Payload, key: string): boolean | undefined {
  const value = payload?.[key];
  return typeof value === 'boolean' ? value : undefined;
}

function readNumber(payload: Payload, key: string): number | undefined {
  const value = payload?.[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function readString(payload: Payload, key: string): string | undefined {
  const value = payload?.[key];
  return typeof value === 'string' ? value : undefined;
}

/** `undefined` when the key is absent; `null` when the core explicitly cleared it. */
function readNullableString(payload: Payload, key: string): string | null | undefined {
  const value = payload?.[key];
  if (typeof value === 'string') {
    return value;
  }
  return value === null ? null : undefined;
}

/**
 * Turn any thrown value into one line of readable text.
 *
 * Shared so every failure channel — core commands and the OS-backed autostart
 * plugin — reports a rejection in the same voice.
 */
export function describeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  try {
    return JSON.stringify(error) ?? String(error);
  } catch {
    return String(error);
  }
}

// -- snapshot patching -------------------------------------------------------

/** Patch the single snapshot store; a no-op while the snapshot is still `null`. */
function patch(update: (snapshot: AppSnapshot) => AppSnapshot): void {
  appState.update((snapshot) => (snapshot ? update(snapshot) : snapshot));
}

function setError(message: string | null): void {
  lastError.set(message);
}

// -- bounded log list --------------------------------------------------------

let nextLogId = 0;

function appendLog(line: string): void {
  const entry: LogEntry = { id: nextLogId, line, receivedAt: Date.now() };
  nextLogId += 1;
  logEntries.update((entries) => {
    const kept =
      entries.length >= LOG_LIMIT ? entries.slice(entries.length - LOG_LIMIT + 1) : entries.slice();
    kept.push(entry);
    return kept;
  });
}

/** Drop every buffered line (Console page "Clear"). */
export function clearLogs(): void {
  logEntries.set([]);
}

// -- snapshot re-read --------------------------------------------------------
//
// `button-changed` and `config-replaced` describe a config change without
// carrying it, so the snapshot is re-read. Refetches are serialized so two
// events in a row cannot resolve out of order and install an older snapshot.

let refreshChain: Promise<void> = Promise.resolve();

async function refreshSnapshot(): Promise<void> {
  appState.set(await api.getStateSnapshot());
}

/** Re-read the snapshot, ignoring failures so they never become unhandled rejections. */
function scheduleRefresh(): Promise<void> {
  refreshChain = refreshChain
    .catch(() => undefined)
    .then(async () => {
      try {
        await refreshSnapshot();
      } catch (error) {
        setError(`Could not refresh state: ${describeError(error)}`);
      }
    });
  return refreshChain;
}

/**
 * Pull the current snapshot on demand (pull-to-refresh, retry buttons).
 */
export function refreshAppState(): Promise<void> {
  return scheduleRefresh();
}

/**
 * Save the working config through the core and re-read the snapshot.
 *
 * The one save path: the Profiles button and the `Ctrl+S` shortcut both call
 * this, so a save can never behave differently depending on how it was
 * triggered. Re-reading after a successful save keeps the path, the active
 * preset and the dirty flag exactly as the core holds them.
 */
export async function saveWorkingConfig(): Promise<boolean> {
  const saved = await runCommand('save configuration', () => api.saveConfig());
  if (saved) {
    await scheduleRefresh();
  }
  return saved;
}

async function refreshPresets(): Promise<void> {
  try {
    presets.set(await api.listPresets());
  } catch (error) {
    setError(`Could not list presets: ${describeError(error)}`);
  }
}

// -- event handlers ----------------------------------------------------------

function onDirtyChanged(payload: AppEventPayloads['octopad://dirty-changed']): void {
  const dirty = readBoolean(asPayload(payload), 'dirty');
  if (dirty === undefined) return;
  patch((snapshot) => (snapshot.dirty === dirty ? snapshot : { ...snapshot, dirty }));
}

function onConfigReplaced(): void {
  void scheduleRefresh();
}

function onButtonChanged(): void {
  // The payload only carries the index; the edited button's data must come
  // from the core.
  void scheduleRefresh();
}

function onTimingChanged(payload: AppEventPayloads['octopad://timing-changed']): void {
  const data = asPayload(payload);
  const field = readString(data, 'field');
  const value = readNumber(data, 'value');
  if (value === undefined) return;
  if (field !== 'debounceMs' && field !== 'clickTimeoutMs' && field !== 'holdTimeoutMs') return;
  const timingField: TimingField = field;
  patch((snapshot) => ({
    ...snapshot,
    config: { ...snapshot.config, [timingField]: value },
  }));
}

function onModeChanged(payload: AppEventPayloads['octopad://mode-changed']): void {
  const modeIndex = readNumber(asPayload(payload), 'mode_index');
  if (modeIndex === undefined) return;
  patch((snapshot) => (snapshot.modeIndex === modeIndex ? snapshot : { ...snapshot, modeIndex }));
}

function onSelectionChanged(payload: AppEventPayloads['octopad://selection-changed']): void {
  const index = readNumber(asPayload(payload), 'index');
  if (index === undefined) return;
  patch((snapshot) =>
    snapshot.selectedIndex === index ? snapshot : { ...snapshot, selectedIndex: index }
  );
}

function onConfigSaved(payload: AppEventPayloads['octopad://config-saved']): void {
  const data = asPayload(payload);
  const ok = readBoolean(data, 'ok');
  const preset = readNullableString(data, 'preset');
  if (ok === false) {
    setError('Could not save the configuration');
    return;
  }
  if (preset) {
    patch((snapshot) => ({ ...snapshot, activePreset: preset }));
  }
}

function onPresetsChanged(): void {
  void refreshPresets();
}

function onSerialStatusChanged(payload: AppEventPayloads['octopad://serial-status-changed']): void {
  const data = asPayload(payload);
  const connected = readBoolean(data, 'connected');
  if (connected === undefined) return;
  const port = readNullableString(data, 'port');
  patch((snapshot) => ({
    ...snapshot,
    serial: { connected, port: port === undefined ? snapshot.serial.port : port },
  }));
}

function onBleStatusChanged(payload: AppEventPayloads['octopad://ble-status-changed']): void {
  const data = asPayload(payload);
  const connected = readBoolean(data, 'connected');
  if (connected === undefined) return;
  const message = readString(data, 'message');
  patch((snapshot) => ({
    ...snapshot,
    ble: { connected, message: message === undefined ? snapshot.ble.message : message },
  }));
}

function onDeviceNameChanged(payload: AppEventPayloads['octopad://device-name-changed']): void {
  const name = readNullableString(asPayload(payload), 'name');
  if (name === undefined) return;
  patch((snapshot) => ({ ...snapshot, device: { ...snapshot.device, name } }));
}

function onDeviceInfoChanged(payload: AppEventPayloads['octopad://device-info-changed']): void {
  const version = readNullableString(asPayload(payload), 'version');
  if (version === undefined) return;
  patch((snapshot) => ({ ...snapshot, device: { ...snapshot.device, firmwareVersion: version } }));
}

function onSerialLog(payload: AppEventPayloads['octopad://serial-log']): void {
  const line = readString(asPayload(payload), 'line');
  if (line === undefined) return;
  appendLog(line);
}

function onHaStatusChanged(payload: AppEventPayloads['octopad://ha-status-changed']): void {
  const data = asPayload(payload);
  const connected = readBoolean(data, 'connected');
  if (connected === undefined) return;
  const message = readString(data, 'message');
  patch((snapshot) => ({
    ...snapshot,
    homeAssistant: {
      ...snapshot.homeAssistant,
      connected,
      message: message === undefined ? snapshot.homeAssistant.message : message,
    },
  }));
}

function onHaEntitiesChanged(payload: AppEventPayloads['octopad://ha-entities-changed']): void {
  const count = readNumber(asPayload(payload), 'count');
  if (count === undefined) return;
  patch((snapshot) => ({ ...snapshot, homeAssistant: { ...snapshot.homeAssistant, entitiesCount: count } }));
}

function onAppsChanged(payload: AppEventPayloads['octopad://apps-changed']): void {
  const count = readNumber(asPayload(payload), 'count');
  if (count === undefined) return;
  patch((snapshot) => ({ ...snapshot, installedAppsCount: count }));
}

/** Registration order is pinned by `state.test.ts`. */
const EVENT_HANDLERS: ReadonlyArray<readonly [api.OctopadEventName, (payload: never) => void]> = [
  [api.OCTOPAD_EVENTS.dirtyChanged, onDirtyChanged],
  [api.OCTOPAD_EVENTS.configReplaced, onConfigReplaced],
  [api.OCTOPAD_EVENTS.buttonChanged, onButtonChanged],
  [api.OCTOPAD_EVENTS.timingChanged, onTimingChanged],
  [api.OCTOPAD_EVENTS.modeChanged, onModeChanged],
  [api.OCTOPAD_EVENTS.selectionChanged, onSelectionChanged],
  [api.OCTOPAD_EVENTS.configSaved, onConfigSaved],
  [api.OCTOPAD_EVENTS.presetsChanged, onPresetsChanged],
  [api.OCTOPAD_EVENTS.serialStatusChanged, onSerialStatusChanged],
  [api.OCTOPAD_EVENTS.bleStatusChanged, onBleStatusChanged],
  [api.OCTOPAD_EVENTS.deviceNameChanged, onDeviceNameChanged],
  [api.OCTOPAD_EVENTS.deviceInfoChanged, onDeviceInfoChanged],
  [api.OCTOPAD_EVENTS.serialLog, onSerialLog],
  [api.OCTOPAD_EVENTS.haStatusChanged, onHaStatusChanged],
  [api.OCTOPAD_EVENTS.haEntitiesChanged, onHaEntitiesChanged],
  [api.OCTOPAD_EVENTS.appsChanged, onAppsChanged],
];

// -- lifecycle ---------------------------------------------------------------

/**
 * Register every core event listener, then fetch and apply the initial
 * snapshot, and return a cleanup that unregisters every listener.
 *
 * Call once from the app shell; the returned function must run on unmount so a
 * remounted page never leaves duplicate listeners behind.
 */
export async function initializeAppState(): Promise<() => void> {
  const unlisteners: api.UnlistenFn[] = [];

  // Phase 1 — listeners first. Each registration is awaited so the snapshot
  // request cannot start while an event is still unobserved.
  await Promise.all(
    EVENT_HANDLERS.map(([event, handler]) =>
      api.listenAppEvent(event, handler as (payload: unknown) => void).then((unlisten) => {
        unlisteners.push(unlisten);
      })
    )
  );

  // Phase 2 — only now ask the core for the read model.
  try {
    await refreshSnapshot();
  } catch (error) {
    // Do not leave listeners attached to a store that never received a
    // snapshot; the caller sees the rejection and can retry.
    for (const unlisten of unlisteners) {
      unlisten();
    }
    throw error;
  }

  let disposed = false;
  return () => {
    if (disposed) return;
    disposed = true;
    for (const unlisten of unlisteners) {
      unlisten();
    }
  };
}

/**
 * Run a command and report whether it actually succeeded.
 *
 * A `false` return means the core rejected the request and a rejection means
 * the command failed outright; both are recorded in `lastError` and reported as
 * `false` so the caller never reports a failed edit or send as successful.
 * Local state is never patched optimistically — it only changes when the core
 * emits an event.
 */
export async function runCommand<T>(label: string, command: () => Promise<T>): Promise<boolean> {
  try {
    const result = await command();
    if (result === false) {
      setError(`${label} was rejected`);
      return false;
    }
    setError(null);
    return true;
  } catch (error) {
    setError(`${label} failed: ${describeError(error)}`);
    return false;
  }
}

/** Reset every store and internal counter (tests, and a full reload). */
export function resetAppState(): void {
  appState.set(null);
  logEntries.set([]);
  presets.set([]);
  lastError.set(null);
  nextLogId = 0;
  refreshChain = Promise.resolve();
  resetEditQueue();
}

/** Read-only aliases for components that only need to consume state. */
export type { Readable };