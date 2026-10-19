/**
 * TypeScript mirror of the Rust wire shape.
 *
 * Nothing here is invented: every key is what `serde` emits for
 * `octopad_core::config` (camelCase) or for `bridge::state_snapshot` /
 * `AppEvent::payload` (flat snake_case event payloads). If a type in this file
 * drifts from the Rust side, the UI silently reads the wrong field — so treat
 * `crates/core/src/state.rs` and `crates/core/src/bridge.rs` as the source of
 * truth and this file as a checked copy.
 */

// -- configuration -----------------------------------------------------------

/**
 * One action in a trigger slot.
 *
 * `Action` is `{"type": ...}` plus a flattened map of every other key, so
 * unknown/extra fields are legal and must survive a round-trip. The index
 * signature is what makes that representable.
 */
export interface Action {
  type: string;
  [field: string]: unknown;
}

/**
 * One trigger slot on disk: a legacy single-object slot, a list of actions, or
 * — for configs that contain neither — a value the core preserves verbatim.
 */
export type TriggerSlot = Action | Action[] | unknown;

/** One physical button within a mode (`octopad_core::config::Button`). */
export interface Button {
  index: number;
  pin: number;
  label: string;
  single: TriggerSlot;
  double: TriggerSlot;
  hold: TriggerSlot;
}

/** A mode layer (desktop, ha, …). */
export interface Mode {
  id: string;
  name: string;
  ledState: boolean;
  buttons: Button[];
}

/**
 * The full configuration (`octopad_core::config::Config`).
 *
 * The three timing fields stay loosely typed on the Rust side (a stored
 * non-numeric value must survive so `timing_command` can reject it), so they
 * are `unknown` here rather than `number`. Use {@link timingValue} to read one
 * safely.
 */
export interface Config {
  version: number;
  schemaVersion: number;
  device: string;
  debounceMs: unknown;
  clickTimeoutMs: unknown;
  holdTimeoutMs: unknown;
  activeModeIndex: number;
  activePreset?: string | null;
  modes: Mode[];
}

/** The only timing keys `set_timing_cmd` accepts. */
export type TimingField = 'debounceMs' | 'clickTimeoutMs' | 'holdTimeoutMs';

/** The only trigger slots every action command accepts. */
export type Trigger = 'single' | 'double' | 'hold';

// -- snapshot ----------------------------------------------------------------

export interface SerialStatus {
  connected: boolean;
  port: string | null;
}

export interface BleStatus {
  connected: boolean;
  message: string;
}

export interface DeviceInfo {
  name: string | null;
  firmwareVersion: string | null;
}

export interface HomeAssistantStatus {
  connected: boolean;
  message: string;
  entitiesCount: number;
}

/** `bridge::state_snapshot` — the whole read model in one object. */
export interface AppSnapshot {
  config: Config;
  configPath: string;
  modeIndex: number;
  selectedIndex: number;
  dirty: boolean;
  activePreset: string | null;
  serial: SerialStatus;
  ble: BleStatus;
  device: DeviceInfo;
  homeAssistant: HomeAssistantStatus;
  installedAppsCount: number;
}

/** `bridge::run_import`'s `ImportReport` (serialized with its snake_case keys). */
export interface ImportReport {
  imported_config: boolean;
  presets: string[];
  warnings: string[];
  first_run_auto: boolean;
}

// -- event payloads ----------------------------------------------------------
//
// Flat snake_case objects, exactly `AppEvent::payload`. Extra keys the core
// may add later are tolerated: the store reads only the keys below.

export interface DirtyChangedPayload {
  dirty: boolean;
}

export interface ConfigReplacedPayload {
  source: string;
}

export interface ButtonChangedPayload {
  index: number;
}

export interface TimingChangedPayload {
  field: string;
  value: number;
}

export interface ModeChangedPayload {
  mode_index: number;
}

export interface SelectionChangedPayload {
  index: number;
}

export interface ConfigSavedPayload {
  ok: boolean;
  preset: string | null;
}

export interface PresetsChangedPayload {
  name: string;
}

export interface SerialStatusChangedPayload {
  connected: boolean;
  port: string | null;
}

export interface BleStatusChangedPayload {
  connected: boolean;
  message: string;
}

export interface DeviceNameChangedPayload {
  name: string | null;
}

export interface DeviceInfoChangedPayload {
  version: string | null;
}

export interface SerialLogPayload {
  line: string;
}

export interface HaStatusChangedPayload {
  connected: boolean;
  message: string;
}

export interface HaEntitiesChangedPayload {
  count: number;
}

export interface AppsChangedPayload {
  count: number;
}

/** Payloads keyed by the event name, so `listen` stays type-safe. */
export interface AppEventPayloads {
  'octopad://dirty-changed': DirtyChangedPayload;
  'octopad://config-replaced': ConfigReplacedPayload;
  'octopad://button-changed': ButtonChangedPayload;
  'octopad://timing-changed': TimingChangedPayload;
  'octopad://mode-changed': ModeChangedPayload;
  'octopad://selection-changed': SelectionChangedPayload;
  'octopad://config-saved': ConfigSavedPayload;
  'octopad://presets-changed': PresetsChangedPayload;
  'octopad://serial-status-changed': SerialStatusChangedPayload;
  'octopad://ble-status-changed': BleStatusChangedPayload;
  'octopad://device-name-changed': DeviceNameChangedPayload;
  'octopad://device-info-changed': DeviceInfoChangedPayload;
  'octopad://serial-log': SerialLogPayload;
  'octopad://ha-status-changed': HaStatusChangedPayload;
  'octopad://ha-entities-changed': HaEntitiesChangedPayload;
  'octopad://apps-changed': AppsChangedPayload;
}

// -- helpers -----------------------------------------------------------------

/**
 * Read a timing as a millisecond count.
 *
 * `Config` keeps the raw JSON value, so a hand-edited `config.json` can hold
 * something that is not a number. `null` means "the core would refuse to sync
 * this", which the UI must show rather than silently coerce.
 */
export function timingValue(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string' && value.trim() !== '') {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}