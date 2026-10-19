export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export interface Action {
  type: string;
  [field: string]: JsonValue;
}

export type TriggerName = 'single' | 'double' | 'hold';
export type TimingField = 'debounceMs' | 'clickTimeoutMs' | 'holdTimeoutMs';

export type ActionSlot = Action | Action[] | JsonValue;

export interface Button {
  index: number;
  pin: number;
  label: string;
  single: ActionSlot;
  double: ActionSlot;
  hold: ActionSlot;
}

export interface Mode {
  id: string;
  name: string;
  ledState: boolean;
  buttons: Button[];
}

export interface Config {
  version: number;
  schemaVersion: number;
  device: string;
  debounceMs: JsonValue;
  clickTimeoutMs: JsonValue;
  holdTimeoutMs: JsonValue;
  activeModeIndex: number;
  activePreset?: string;
  modes: Mode[];
}

export interface AppSnapshot {
  config: Config;
  configPath: string;
  modeIndex: number;
  selectedIndex: number;
  dirty: boolean;
  activePreset: string | null;
  serial: {
    connected: boolean;
    port: string | null;
  };
  ble: {
    connected: boolean;
    message: string;
  };
  device: {
    name: string | null;
    firmwareVersion: string | null;
  };
  homeAssistant: {
    connected: boolean;
    message: string;
    entitiesCount: number;
  };
  installedAppsCount: number;
}

export interface DirtyChangedPayload {
  dirty: boolean;
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

export interface AppEventPayloadMap {
  'octopad://dirty-changed': DirtyChangedPayload;
  'octopad://button-changed': ButtonChangedPayload;
  'octopad://timing-changed': TimingChangedPayload;
  'octopad://mode-changed': ModeChangedPayload;
  'octopad://selection-changed': SelectionChangedPayload;
  'octopad://serial-status-changed': SerialStatusChangedPayload;
  'octopad://ble-status-changed': BleStatusChangedPayload;
  'octopad://device-name-changed': DeviceNameChangedPayload;
  'octopad://device-info-changed': DeviceInfoChangedPayload;
  'octopad://serial-log': SerialLogPayload;
}

export interface TimestampedLogLine {
  line: string;
  timestamp: number;
}

export interface AppStoreState {
  snapshot: AppSnapshot | null;
  logs: TimestampedLogLine[];
}
