// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest';
import { handleAppShortcut } from './shortcuts';

function keydown(target: HTMLElement, key: string, ctrlKey = true): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    key,
    ctrlKey,
    bubbles: true,
    cancelable: true
  });
  target.dispatchEvent(event);
  return event;
}

describe('application keyboard shortcuts', () => {
  it('handles Ctrl+S as Save and prevents the browser default', () => {
    const onSave = vi.fn();
    const onQuit = vi.fn();
    const event = keydown(document.body, 's');

    expect(handleAppShortcut(event, onSave, onQuit)).toBe(true);
    expect(event.defaultPrevented).toBe(true);
    expect(onSave).toHaveBeenCalledOnce();
    expect(onQuit).not.toHaveBeenCalled();
  });

  it('handles Ctrl+Q as Quit and prevents the browser default', () => {
    const onSave = vi.fn();
    const onQuit = vi.fn();
    const event = keydown(document.body, 'q');

    expect(handleAppShortcut(event, onSave, onQuit)).toBe(true);
    expect(event.defaultPrevented).toBe(true);
    expect(onQuit).toHaveBeenCalledOnce();
    expect(onSave).not.toHaveBeenCalled();
  });

  it('leaves text editing shortcuts untouched in text inputs and textareas', () => {
    const onSave = vi.fn();
    const onQuit = vi.fn();
    const input = document.createElement('input');
    const inputEvent = keydown(input, 's');
    const textarea = document.createElement('textarea');
    const textareaEvent = keydown(textarea, 'q');

    expect(handleAppShortcut(inputEvent, onSave, onQuit)).toBe(false);
    expect(handleAppShortcut(textareaEvent, onSave, onQuit)).toBe(false);
    expect(inputEvent.defaultPrevented).toBe(false);
    expect(textareaEvent.defaultPrevented).toBe(false);
    expect(onSave).not.toHaveBeenCalled();
    expect(onQuit).not.toHaveBeenCalled();
  });

  it('ignores unrelated keys and shortcuts without Ctrl', () => {
    const onSave = vi.fn();
    const onQuit = vi.fn();
    const unrelated = keydown(document.body, 'x');
    const withoutCtrl = keydown(document.body, 's', false);

    expect(handleAppShortcut(unrelated, onSave, onQuit)).toBe(false);
    expect(handleAppShortcut(withoutCtrl, onSave, onQuit)).toBe(false);
    expect(onSave).not.toHaveBeenCalled();
    expect(onQuit).not.toHaveBeenCalled();
  });
});
