/**
 * Contract tests for the toast pipe (`toast.ts`).
 *
 * A toast is feedback whose result is only ever informational — action test
 * results in M3, later flows after that — so the store stays deliberately
 * minimal: append, auto-expire after ~4 s, and a manual `dismiss` for the
 * host's own close affordance. Edits have the other failure channels
 * (`lastError`, the error banner); nothing here is allowed to grow into a
 * second one.
 *
 * Every expiry runs on fake timers so the ~4 s contract is exact without the
 * suite ever actually waiting, in the same spirit as the faked `Date` in the
 * console-page tests.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

import { TOAST_DURATION_MS, dismiss, resetToasts, show, toasts } from './toast';

beforeEach(() => {
  // Only the timer calls are faked, so promises and everything else stay real.
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
  resetToasts();
});

afterEach(() => {
  resetToasts();
  vi.useRealTimers();
});

describe('toast store', () => {
  it('appends toasts with their message and kind', () => {
    show('Test: F1 sent', 'success');
    show('No actions on this trigger', 'info');
    show('Test: no serial', 'error');

    expect(get(toasts).map((toast) => [toast.message, toast.kind])).toEqual([
      ['Test: F1 sent', 'success'],
      ['No actions on this trigger', 'info'],
      ['Test: no serial', 'error'],
    ]);
  });

  it('defaults to the info kind', () => {
    show('Scanning entities…');

    expect(get(toasts)[0].kind).toBe('info');
  });

  it('gives every toast a distinct id the host can key on', () => {
    const first = show('one');
    const second = show('two');

    expect(first).not.toBe(second);
    expect(get(toasts).map((toast) => toast.id)).toEqual([first, second]);
  });

  it('expires a toast after ~4 s, not a moment earlier', () => {
    show('Test: F1 sent', 'success');
    expect(get(toasts)).toHaveLength(1);

    vi.advanceTimersByTime(TOAST_DURATION_MS - 1);
    expect(get(toasts)).toHaveLength(1);

    vi.advanceTimersByTime(1);
    expect(get(toasts)).toHaveLength(0);
  });

  it('expires each toast on its own timer, in push order', () => {
    show('first', 'success');
    vi.advanceTimersByTime(1000);
    show('second', 'error');

    vi.advanceTimersByTime(TOAST_DURATION_MS - 1000);
    expect(get(toasts).map((toast) => toast.message)).toEqual(['second']);

    vi.advanceTimersByTime(1000);
    expect(get(toasts)).toHaveLength(0);
  });

  it('dismiss removes one toast early without touching the others', () => {
    show('keep', 'success');
    const gone = show('gone', 'error');

    dismiss(gone);
    expect(get(toasts).map((toast) => toast.message)).toEqual(['keep']);

    // The survivor still expires on schedule.
    vi.advanceTimersByTime(TOAST_DURATION_MS);
    expect(get(toasts)).toHaveLength(0);
  });

  it('dismiss of an unknown id is a no-op', () => {
    show('only', 'info');

    expect(() => dismiss(9999)).not.toThrow();
    expect(get(toasts)).toHaveLength(1);
  });
});
