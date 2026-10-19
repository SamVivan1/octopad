/**
 * The app-wide toast pipe: short-lived feedback for results that are purely
 * informational — an action test's outcome today, later M3 flows after that.
 *
 * This is deliberately minimal ({ message, kind } entries with an id for the
 * host's keyed `#each`), and deliberately separate from the failure channels:
 *
 * - `lastError` in `./state` is for *config edits and device changes*, which
 *   must stay on screen until the next command succeeds.
 * - A toast is feedback for an execution whose result cannot be acted on —
 *   the old app showed exactly these as Adw toasts ("Test: {result}"), so the
 *   rewrite keeps them as pop-ups that expire on their own (~4 s).
 *
 * Components only ever call `show()`; expiry and clearing are the store's own
 * business, so a component cannot forget to cancel a timer.
 */

import { writable } from 'svelte/store';
import type { Writable } from 'svelte/store';

/** Green ("success"), red ("error") or neutral ("info") toasts. */
export type ToastKind = 'success' | 'error' | 'info';

/** One toast: what it says, which colour it takes, and its list identity. */
export interface Toast {
  id: number;
  message: string;
  kind: ToastKind;
}

/** How long a toast stays on screen before it removes itself. */
export const TOAST_DURATION_MS = 4000;

/** Current toasts, oldest first — `ToastHost` renders exactly this. */
export const toasts: Writable<Toast[]> = writable<Toast[]>([]);

let nextId = 0;
const expiryTimers = new Map<number, ReturnType<typeof setTimeout>>();

/**
 * Push a toast and return its id (`dismiss` accepts it for an early close).
 *
 * The kind defaults to `info`, matching the old app's `show_toast(message)`.
 */
export function show(message: string, kind: ToastKind = 'info'): number {
  const id = nextId;
  nextId += 1;
  toasts.update((list) => [...list, { id, message, kind }]);
  expiryTimers.set(
    id,
    setTimeout(() => dismiss(id), TOAST_DURATION_MS)
  );
  return id;
}

/** Remove one toast now (explicit close) or when its own timer fires. */
export function dismiss(id: number): void {
  const timer = expiryTimers.get(id);
  if (timer !== undefined) {
    clearTimeout(timer);
    expiryTimers.delete(id);
  }
  toasts.update((list) => list.filter((toast) => toast.id !== id));
}

/** Drop every toast and reset the identity counter (tests, and a page reload). */
export function resetToasts(): void {
  for (const timer of expiryTimers.values()) {
    clearTimeout(timer);
  }
  expiryTimers.clear();
  nextId = 0;
  toasts.set([]);
}
