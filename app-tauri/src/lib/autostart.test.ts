/**
 * Contract tests for the autostart state layer (Task 8).
 *
 * Autostart lives entirely in the OS (a Windows `Run` registry value, a Linux
 * autostart desktop entry), so the frontend never keeps its own copy of the
 * flag: it reads the platform through `tauri-plugin-autostart`, writes it, and
 * then **re-reads it** so the UI can only ever show what the OS actually says.
 *
 * What is pinned here:
 *
 * - A successful read publishes the platform state.
 * - A failed read publishes *nothing* (the state stays unknown) and records the
 *   failure — the toggle must never claim a value the OS did not report.
 * - A write calls exactly one of `enable`/`disable` and only reports success
 *   after the confirming re-read matches what was asked for.
 * - A rejected write is reported as a failure, and the re-read still runs so
 *   the UI falls back to the real platform state instead of the optimistic one.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const { enableMock, disableMock, isEnabledMock } = vi.hoisted(() => ({
  enableMock: vi.fn(),
  disableMock: vi.fn(),
  isEnabledMock: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-autostart', () => ({
  enable: enableMock,
  disable: disableMock,
  isEnabled: isEnabledMock,
}));

import {
  autostartEnabled,
  readAutostart,
  resetAutostart,
  setAutostart,
} from './autostart';
import { lastError, resetAppState } from './state';

/** A platform that reports `enabled` and records writes in `platformState`. */
function fakePlatform(enabled: boolean): { state: boolean } {
  const platform = { state: enabled };
  isEnabledMock.mockImplementation(async () => platform.state);
  enableMock.mockImplementation(async () => {
    platform.state = true;
  });
  disableMock.mockImplementation(async () => {
    platform.state = false;
  });
  return platform;
}

beforeEach(() => {
  enableMock.mockReset();
  disableMock.mockReset();
  isEnabledMock.mockReset();
  resetAppState();
  resetAutostart();
});

describe('readAutostart', () => {
  it('publishes the platform state and reports success', async () => {
    fakePlatform(true);

    await expect(readAutostart()).resolves.toBe(true);

    expect(isEnabledMock).toHaveBeenCalledTimes(1);
    expect(get(autostartEnabled)).toBe(true);
    expect(get(lastError)).toBeNull();
  });

  it('publishes a disabled platform state just the same', async () => {
    fakePlatform(false);

    await expect(readAutostart()).resolves.toBe(false);

    expect(get(autostartEnabled)).toBe(false);
    expect(get(lastError)).toBeNull();
  });

  it('leaves the state unknown and records the failure when the read fails', async () => {
    fakePlatform(false);
    isEnabledMock.mockRejectedValue(new Error('registry unavailable'));

    await expect(readAutostart()).resolves.toBe(false);

    expect(get(autostartEnabled)).toBeNull();
    expect(get(lastError)).toMatch(/autostart/i);
  });
});

describe('setAutostart', () => {
  it('enables the platform entry and confirms it with a re-read', async () => {
    const platform = fakePlatform(false);

    await expect(setAutostart(true)).resolves.toBe(true);

    expect(enableMock).toHaveBeenCalledTimes(1);
    expect(disableMock).not.toHaveBeenCalled();
    expect(platform.state).toBe(true);
    expect(isEnabledMock).toHaveBeenCalledTimes(1);
    expect(get(autostartEnabled)).toBe(true);
    expect(get(lastError)).toBeNull();
  });

  it('disables the platform entry and confirms it with a re-read', async () => {
    const platform = fakePlatform(true);

    await expect(setAutostart(false)).resolves.toBe(true);

    expect(disableMock).toHaveBeenCalledTimes(1);
    expect(enableMock).not.toHaveBeenCalled();
    expect(platform.state).toBe(false);
    expect(get(autostartEnabled)).toBe(false);
    expect(get(lastError)).toBeNull();
  });

  it('reports failure when the platform does not reach the requested state', async () => {
    fakePlatform(false);
    // A write that silently does nothing must not be reported as success.
    enableMock.mockImplementation(async () => undefined);

    await expect(setAutostart(true)).resolves.toBe(false);

    expect(get(autostartEnabled)).toBe(false);
    expect(get(lastError)).toMatch(/autostart/i);
  });

  it('reports a rejected write and falls back to the real platform state', async () => {
    fakePlatform(false);
    enableMock.mockRejectedValue(new Error('access denied'));

    await expect(setAutostart(true)).resolves.toBe(false);

    expect(get(autostartEnabled)).toBe(false);
    expect(get(lastError)).toMatch(/autostart/i);
  });

  it('reports a rejected disable and keeps the platform state that is still on', async () => {
    fakePlatform(true);
    disableMock.mockRejectedValue(new Error('access denied'));

    await expect(setAutostart(false)).resolves.toBe(false);

    expect(get(autostartEnabled)).toBe(true);
    expect(get(lastError)).toMatch(/autostart/i);
  });
});

describe('resetAutostart', () => {
  it('returns the state to unknown', async () => {
    fakePlatform(true);
    await readAutostart();
    expect(get(autostartEnabled)).toBe(true);

    resetAutostart();

    expect(get(autostartEnabled)).toBeNull();
  });
});