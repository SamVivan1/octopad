/**
 * Autostart ("launch at login") state, read from and written to the OS.
 *
 * The flag itself lives outside the process — a Windows `Run` registry value, a
 * Linux autostart desktop entry — so this module holds **no** authoritative
 * copy of it. `autostartEnabled` is only ever a mirror of what the platform
 * last reported, and `null` means "not known": the read failed, so the toggle
 * must not claim a state (Global Constraint: a failure must never be presented
 * as a successful change).
 *
 * Every write is confirmed by re-reading: the plugin call resolving does not
 * mean the OS entry was created, and `enable()` on an unsupported host can
 * quietly do nothing. A write is reported as successful only when the
 * confirming read matches what was asked for.
 */

import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart';
import { writable } from 'svelte/store';
import type { Writable } from 'svelte/store';

import { describeError, lastError, runCommand } from './state';

/**
 * The platform's autostart state, or `null` while it is unknown.
 *
 * `null` is what the Settings toggle renders as "unavailable" with the control
 * disabled, so a failed read can never leave a switch in a made-up position.
 */
export const autostartEnabled: Writable<boolean | null> = writable<boolean | null>(null);

/** Read the platform state, or `null` when it could not be read. */
async function readPlatform(): Promise<boolean | null> {
  try {
    return await isEnabled();
  } catch (error) {
    lastError.set(`Could not read the autostart state: ${describeError(error)}`);
    return null;
  }
}

/**
 * Read the platform autostart state and publish it.
 *
 * Returns the state, or `false` when the read failed — in which case the
 * published state is left unknown (`null`) and the failure is recorded in
 * `lastError` for the UI to surface.
 */
export async function readAutostart(): Promise<boolean> {
  const enabled = await readPlatform();
  autostartEnabled.set(enabled);
  return enabled ?? false;
}

/**
 * Turn the platform login entry on or off, then confirm by re-reading.
 *
 * Returns `true` only when the platform reports the requested state. A rejected
 * write, or a write the platform silently ignored, is recorded in `lastError`
 * and reported as `false`, and the published state is the one the platform
 * actually has.
 */
export async function setAutostart(enabled: boolean): Promise<boolean> {
  const written = await runCommand(enabled ? 'enable autostart' : 'disable autostart', () =>
    enabled ? enable() : disable()
  );

  const actual = await readPlatform();
  autostartEnabled.set(actual);

  if (!written) {
    return false;
  }
  if (actual !== enabled) {
    lastError.set(
      `Autostart is still ${actual ? 'on' : 'off'}; the login entry was not changed.`
    );
    return false;
  }
  return true;
}

/** Forget the mirrored state (tests, and a full reload). */
export function resetAutostart(): void {
  autostartEnabled.set(null);
}