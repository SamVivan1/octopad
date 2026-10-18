import { writable } from 'svelte/store';
import {
  APP_EVENT_NAMES,
  getStateSnapshot,
  listenAppEvent,
  type AppEventName
} from './api';
import type { AppSnapshot, AppStoreState, TimestampedLogLine } from './types';

const MAX_LOG_LINES = 1500;
const TRIM_LOG_LINES = 200;

export const appState = writable<AppStoreState>({ snapshot: null, logs: [] });

function recordOf(payload: unknown): Record<string, unknown> | null {
  return typeof payload === 'object' && payload !== null
    ? (payload as Record<string, unknown>)
    : null;
}

function appendLog(
  logs: TimestampedLogLine[],
  line: string
): TimestampedLogLine[] {
  const next = [...logs, { line, timestamp: Date.now() }];
  return next.length > MAX_LOG_LINES ? next.slice(TRIM_LOG_LINES) : next;
}

function updateFromEvent(name: AppEventName, payload: unknown): void {
  const data = recordOf(payload);
  if (!data) return;

  appState.update((current) => {
    if (name === 'octopad://serial-log') {
      return typeof data.line === 'string'
        ? { ...current, logs: appendLog(current.logs, data.line) }
        : current;
    }

    const snapshot = current.snapshot;
    if (!snapshot) return current;

    switch (name) {
      case 'octopad://dirty-changed':
        return typeof data.dirty === 'boolean'
          ? { ...current, snapshot: { ...snapshot, dirty: data.dirty } }
          : current;
      case 'octopad://mode-changed':
        return typeof data.mode_index === 'number'
          ? {
              ...current,
              snapshot: {
                ...snapshot,
                modeIndex: data.mode_index,
                config: { ...snapshot.config, activeModeIndex: data.mode_index }
              }
            }
          : current;
      case 'octopad://selection-changed':
        return typeof data.index === 'number'
          ? { ...current, snapshot: { ...snapshot, selectedIndex: data.index } }
          : current;
      case 'octopad://serial-status-changed':
        return typeof data.connected === 'boolean' &&
          (typeof data.port === 'string' || data.port === null)
          ? {
              ...current,
              snapshot: {
                ...snapshot,
                serial: { connected: data.connected, port: data.port }
              }
            }
          : current;
      case 'octopad://ble-status-changed':
        return typeof data.connected === 'boolean' && typeof data.message === 'string'
          ? {
              ...current,
              snapshot: {
                ...snapshot,
                ble: { connected: data.connected, message: data.message }
              }
            }
          : current;
      case 'octopad://device-name-changed':
        return typeof data.name === 'string' || data.name === null
          ? {
              ...current,
              snapshot: {
                ...snapshot,
                device: { ...snapshot.device, name: data.name }
              }
            }
          : current;
      case 'octopad://device-info-changed':
        return typeof data.version === 'string' || data.version === null
          ? {
              ...current,
              snapshot: {
                ...snapshot,
                device: { ...snapshot.device, firmwareVersion: data.version }
              }
            }
          : current;
      case 'octopad://button-changed':
      case 'octopad://timing-changed':
        return current;
    }
  });
}

let snapshotRequest = 0;

export async function refreshAppSnapshot(): Promise<void> {
  const request = ++snapshotRequest;
  const snapshot = await getStateSnapshot();
  if (request === snapshotRequest) {
    appState.update((current) => ({ ...current, snapshot }));
  }
}

export async function initializeAppState(): Promise<() => void> {
  const unlisten: Array<() => void> = [];

  try {
    for (const eventName of APP_EVENT_NAMES) {
      unlisten.push(
        await listenAppEvent(eventName, (payload) => updateFromEvent(eventName, payload))
      );
    }
    await refreshAppSnapshot();
  } catch (error) {
    for (const stopListening of unlisten) stopListening();
    throw error;
  }

  let cleaned = false;
  return () => {
    if (cleaned) return;
    cleaned = true;
    for (const stopListening of unlisten) stopListening();
  };
}
