// @vitest-environment jsdom
/**
 * Contract tests for the app keyboard shortcuts (Task 8).
 *
 * The shortcut layer is a pure decision function plus a thin installer, so the
 * rules can be pinned without a running Tauri window:
 *
 * - **Ctrl+S saves and Ctrl+Q quits** while the OctoPad window is focused.
 * - **Nothing else is a shortcut.** Plain keys and chords with extra modifiers
 *   (`Ctrl+Shift+S`, `Ctrl+Alt+Q`, `Meta`) must fall through to the webview, so
 *   `Ctrl+Shift+S` stays free for the M3 "save as preset" binding.
 * - **Text editing wins.** If the event target is an `<input>`, `<textarea>`,
 *   `<select>` or anything inside a `contenteditable` region, the shortcut is
 *   ignored and the keystroke is left to normal editing — a Ctrl+S typed into
 *   the device-name field must never be hijacked into a save.
 * - **An IME composition is never hijacked** either: `isComposing` keystrokes
 *   belong to the input method.
 * - A handled shortcut calls `preventDefault` exactly once and runs exactly one
 *   binding; an ignored one touches nothing.
 */

import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  handleShortcut,
  installShortcutHandler,
  isEditableTarget,
  shortcutFor,
} from './shortcuts';
import type { ShortcutBindings, ShortcutEventLike } from './shortcuts';

type Overrides = Partial<ShortcutEventLike>;

function keyEvent(overrides: Overrides = {}): ShortcutEventLike {
  const event: ShortcutEventLike = {
    key: '',
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    shiftKey: false,
    isComposing: false,
    preventDefault: vi.fn(),
    target: null,
    ...overrides,
  };
  return event;
}

function bindings(): ShortcutBindings & {
  save: ReturnType<typeof vi.fn<() => void>>;
  quit: ReturnType<typeof vi.fn<() => void>>;
} {
  return { save: vi.fn(), quit: vi.fn() };
}

function element(html: string): HTMLElement {
  const host = document.createElement('div');
  host.innerHTML = html;
  document.body.append(host);
  return host.firstElementChild as HTMLElement;
}

afterEach(() => {
  document.body.innerHTML = '';
});

// -- the chords ---------------------------------------------------------------

describe('shortcutFor', () => {
  it('maps Ctrl+S to save and Ctrl+Q to quit', () => {
    expect(shortcutFor(keyEvent({ key: 's', ctrlKey: true }))).toBe('save');
    expect(shortcutFor(keyEvent({ key: 'S', ctrlKey: true }))).toBe('save');
    expect(shortcutFor(keyEvent({ key: 'q', ctrlKey: true }))).toBe('quit');
    expect(shortcutFor(keyEvent({ key: 'Q', ctrlKey: true }))).toBe('quit');
  });

  it('ignores unmodified keys and chords carrying extra modifiers', () => {
    expect(shortcutFor(keyEvent({ key: 's' }))).toBeNull();
    expect(shortcutFor(keyEvent({ key: 'q' }))).toBeNull();
    expect(shortcutFor(keyEvent({ key: 'S', ctrlKey: true, shiftKey: true }))).toBeNull();
    expect(shortcutFor(keyEvent({ key: 'Q', ctrlKey: true, altKey: true }))).toBeNull();
    expect(shortcutFor(keyEvent({ key: 's', metaKey: true }))).toBeNull();
    expect(shortcutFor(keyEvent({ key: 'Enter', ctrlKey: true }))).toBeNull();
  });

  it('ignores a keystroke that belongs to an IME composition', () => {
    expect(shortcutFor(keyEvent({ key: 's', ctrlKey: true, isComposing: true }))).toBeNull();
  });
});

// -- text editing is never hijacked -------------------------------------------

describe('isEditableTarget', () => {
  it('treats inputs, textareas, selects and contenteditable as editing targets', () => {
    expect(isEditableTarget(element('<input type="text" />'))).toBe(true);
    expect(isEditableTarget(element('<textarea></textarea>'))).toBe(true);
    expect(isEditableTarget(element('<select><option>a</option></select>'))).toBe(true);
    expect(isEditableTarget(element('<div contenteditable="true"></div>'))).toBe(true);
    expect(isEditableTarget(element('<span contenteditable="true"></span>'))).toBe(true);
  });

  it('treats ordinary app chrome as not editable', () => {
    expect(isEditableTarget(element('<button type="button">Save</button>'))).toBe(false);
    expect(isEditableTarget(element('<div class="card"></div>'))).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });
});

describe('shortcutFor inside a focused text field', () => {
  it('does not hijack Ctrl+S or Ctrl+Q typed into a form field', () => {
    const input = element('<input type="text" />');
    expect(shortcutFor(keyEvent({ key: 's', ctrlKey: true, target: input }))).toBeNull();
    expect(shortcutFor(keyEvent({ key: 'q', ctrlKey: true, target: input }))).toBeNull();
  });

  it('does not hijack keystrokes inside a contenteditable region', () => {
    const host = element('<div contenteditable="true"><span id="inner">text</span></div>');
    const inner = host.querySelector('#inner') as HTMLElement;
    expect(shortcutFor(keyEvent({ key: 's', ctrlKey: true, target: inner }))).toBeNull();
  });

  it('falls back to the focused element when the event carries no target', () => {
    const input = element('<input type="text" />');
    input.focus();
    expect(shortcutFor(keyEvent({ key: 's', ctrlKey: true, target: null }))).toBeNull();

    input.blur();
    expect(document.activeElement).not.toBe(input);
    expect(shortcutFor(keyEvent({ key: 's', ctrlKey: true, target: null }))).toBe('save');
  });
});

// -- dispatching --------------------------------------------------------------

describe('handleShortcut', () => {
  it('runs exactly one binding and prevents the default for Ctrl+S', () => {
    const handlers = bindings();
    const event = keyEvent({ key: 's', ctrlKey: true });

    expect(handleShortcut(event, handlers)).toBe(true);
    expect(handlers.save).toHaveBeenCalledTimes(1);
    expect(handlers.quit).not.toHaveBeenCalled();
    expect(event.preventDefault).toHaveBeenCalledTimes(1);
  });

  it('runs the quit binding for Ctrl+Q', () => {
    const handlers = bindings();
    const event = keyEvent({ key: 'q', ctrlKey: true });

    expect(handleShortcut(event, handlers)).toBe(true);
    expect(handlers.quit).toHaveBeenCalledTimes(1);
    expect(handlers.save).not.toHaveBeenCalled();
    expect(event.preventDefault).toHaveBeenCalledTimes(1);
  });

  it('leaves an ignored keystroke completely alone', () => {
    const handlers = bindings();
    const event = keyEvent({ key: 's', ctrlKey: true, target: element('<textarea></textarea>') });

    expect(handleShortcut(event, handlers)).toBe(false);
    expect(handlers.save).not.toHaveBeenCalled();
    expect(handlers.quit).not.toHaveBeenCalled();
    expect(event.preventDefault).not.toHaveBeenCalled();
  });
});

// -- installation -------------------------------------------------------------

describe('installShortcutHandler', () => {
  function press(init: KeyboardEventInit, node: EventTarget = window): KeyboardEvent {
    const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
    node.dispatchEvent(event);
    return event;
  }

  it('saves on Ctrl+S and quits on Ctrl+Q while the window is focused', () => {
    const handlers = bindings();
    const uninstall = installShortcutHandler(handlers);

    const save = press({ key: 's', ctrlKey: true });
    expect(handlers.save).toHaveBeenCalledTimes(1);
    expect(save.defaultPrevented).toBe(true);

    press({ key: 'q', ctrlKey: true });
    expect(handlers.quit).toHaveBeenCalledTimes(1);

    uninstall();
  });

  it('does not hijack a Ctrl+S typed into a text field inside the app', () => {
    const handlers = bindings();
    const uninstall = installShortcutHandler(handlers);
    const input = element('<input type="text" />');

    const event = press({ key: 's', ctrlKey: true }, input);

    expect(handlers.save).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);

    uninstall();
  });

  it('stops handling keys after the returned cleanup runs', () => {
    const handlers = bindings();
    const uninstall = installShortcutHandler(handlers);
    uninstall();

    press({ key: 's', ctrlKey: true });

    expect(handlers.save).not.toHaveBeenCalled();
  });
});