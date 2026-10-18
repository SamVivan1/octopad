import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  Action,
  AppEventPayloadMap,
  AppSnapshot
} from './types';

export const APP_EVENT_NAMES = [
  'octopad://dirty-changed',
  'octopad://button-changed',
  'octopad://timing-changed',
  'octopad://mode-changed',
  'octopad://selection-changed',
  'octopad://serial-status-changed',
  'octopad://ble-status-changed',
  'octopad://device-name-changed',
  'octopad://device-info-changed',
  'octopad://serial-log'
] as const;

export type AppEventName = (typeof APP_EVENT_NAMES)[number];

export function getStateSnapshot(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('get_state_snapshot');
}

export function listenAppEvent<K extends AppEventName>(
  event: K,
  handler: (payload: AppEventPayloadMap[K]) => void
): Promise<UnlistenFn> {
  return listen<AppEventPayloadMap[K]>(event, ({ payload }) => handler(payload));
}

export function setMode(index: number): Promise<boolean> {
  return invoke<boolean>('set_mode_cmd', { index });
}

export function selectButton(index: number): Promise<boolean> {
  return invoke<boolean>('select_button_cmd', { index });
}

export function setLabel(text: string): Promise<void> {
  return invoke<void>('set_label_cmd', { text });
}

export function setAction(trigger: string, index: number, action: Action): Promise<void> {
  return invoke<void>('set_action_cmd', { trigger, index, action });
}

export function addAction(trigger: string): Promise<void> {
  return invoke<void>('add_action_cmd', { trigger });
}

export function removeAction(trigger: string, index: number): Promise<void> {
  return invoke<void>('remove_action_cmd', { trigger, index });
}

export function moveAction(trigger: string, index: number, delta: number): Promise<void> {
  return invoke<void>('move_action_cmd', { trigger, index, delta });
}

export function setTiming(field: string, value: number): Promise<boolean> {
  return invoke<boolean>('set_timing_cmd', { field, value });
}

export function sendDeviceCommand(command: string): Promise<boolean> {
  return invoke<boolean>('send_device_command_cmd', { command });
}

export function saveConfig(): Promise<boolean> {
  return invoke<boolean>('save_config_cmd');
}

export function renameDevice(name: string): Promise<boolean> {
  return invoke<boolean>('rename_device_cmd', { name });
}

export function importLegacyConfig(): Promise<unknown> {
  return invoke<unknown>('import_legacy_config');
}
