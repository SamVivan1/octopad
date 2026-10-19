/**
 * App-level keyboard shortcuts (Task 8).
 *
 * The shortcut logic is a **pure decision function** (`shortcutFor`) plus a thin
 * DOM installer, so the rules are pinned by `shortcuts.test.ts` without a
 * running Tauri window and nothing here talks to the core.
 *
 * ## The rule, in one sentence
 *
 * `Ctrl+S` saves and `Ctrl+Q` quits **while the OctoPad window is focused and
 * focus is not inside an editing surface** — `<input>`, `<textarea>`,
 * `<select>`, or anything inside a `contenteditable` region. Inside such a field
 * the keystroke belongs to normal text editing and is left untouched.
 *
 * Why that direction: the M2 editing surfaces are a device-name field, timing
 * inputs, the console command line and free-text action fields (script text,
 * HA data). A user editing a script has no reason to expect Ctrl+S to write the
 * config file *and* lose the keystroke out of the field, and a hijacked
 * shortcut that also swallows the character is the more surprising failure. The
 * cost of the rule is one `Escape`/click-away before the shortcuts work again
 * inside a field, which is invisible and recoverable.
 *
 * Two further exclusions, both about text entry rather than about the chord:
 *
 * - **Chords with extra modifiers are not shortcuts.** Only exactly `Ctrl+S`
 *   and `Ctrl+Q` bind, so `Ctrl+Shift+S` remains free for the M3
 *   "save as preset" binding, and `Meta` is left alone because macOS is out of
 *   scope for M2.
 * - **IME compositions are never hijacked.** `event.isComposing` belongs to the
 *   input method, whatever the key looks like.
 *
 * An ignored keystroke is a complete no-op: no binding runs and
 * `preventDefault` is not called, so the webview's own behavior is unchanged.
 */

export type ShortcutAction = 'save' | 'quit';

/**
 * The subset of `KeyboardEvent` the decision needs.
 *
 * Structural rather than the DOM type so tests can pass plain objects, and so
 * an event arriving from any source (real DOM, future WebView quirks) is judged
 * by the same code.
 */
export interface ShortcutEventLike {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  isComposing?: boolean;
  preventDefault?: () => void;
  target?: EventTarget | null;
}

/** What the shortcuts actually do; supplied by the app shell. */
export interface ShortcutBindings {
  /** `Ctrl+S` — runs the same save path as the Profiles Save button. */
  save: () => void;
  /** `Ctrl+Q` — signals runtime shutdown and exits, like the tray Quit item. */
  quit: () => void;
}

/** Tags whose content is edited by typing. */
const EDITABLE_TAGS: ReadonlySet<string> = new Set(['INPUT', 'TEXTAREA', 'SELECT']);

const CONTENTEDITABLE_SELECTOR = '[contenteditable]';

function tagNameOf(target: unknown): string | null {
  if (target !== null && typeof target === 'object' && 'tagName' in target) {
    return String((target as { tagName: unknown }).tagName).toUpperCase();
  }
  return null;
}

/**
 * Whether the target is an editing surface.
 *
 * `isContentEditable` is preferred because it is already true for every
 * element inside a `contenteditable` region; the `closest` check is the
 * fallback for hosts that do not implement it, and also covers a plain
 * `contenteditable` attribute without the property.
 */
export function isEditableTarget(target: EventTarget | null | undefined): boolean {
  if (target === null || target === undefined || typeof target !== 'object') {
    return false;
  }
  if (EDITABLE_TAGS.has(tagNameOf(target) ?? '')) {
    return true;
  }
  const candidate = target as Partial<Element> & { isContentEditable?: boolean };
  if (candidate.isContentEditable === true) {
    return true;
  }
  if (typeof candidate.closest === 'function') {
    return candidate.closest(CONTENTEDITABLE_SELECTOR) !== null;
  }
  return false;
}

/**
 * The element that has focus for this event.
 *
 * `event.target` is the focused element for a real `keydown`, but a synthetic
 * event may not carry one, so the document's active element is the fallback.
 */
function focusTargetOf(event: ShortcutEventLike): EventTarget | null {
  if (event.target) {
    return event.target;
  }
  if (typeof document === 'undefined') {
    return null;
  }
  return document.activeElement ?? null;
}

/**
 * Which action a keystroke maps to, or `null` when it is not one of ours.
 *
 * Pure: nothing is invoked, nothing is prevented. See the module comment for
 * the exact rule.
 */
export function shortcutFor(event: ShortcutEventLike): ShortcutAction | null {
  if (!event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) {
    return null;
  }
  if (event.isComposing === true) {
    return null;
  }
  if (isEditableTarget(focusTargetOf(event))) {
    return null;
  }
  const key = typeof event.key === 'string' ? event.key.toLowerCase() : '';
  if (key === 's') {
    return 'save';
  }
  if (key === 'q') {
    return 'quit';
  }
  return null;
}

/**
 * Run the binding for a keystroke.
 *
 * Returns `true` when the event was a shortcut that has been handled — and
 * only then is `preventDefault()` called — and `false` when the event was left
 * entirely to the webview.
 */
export function handleShortcut(event: ShortcutEventLike, bindings: ShortcutBindings): boolean {
  const action = shortcutFor(event);
  if (action === null) {
    return false;
  }
  event.preventDefault?.();
  if (action === 'save') {
    bindings.save();
  } else {
    bindings.quit();
  }
  return true;
}

/** The subset of `Window` the installer touches. */
export interface ShortcutTarget {
  addEventListener(type: string, listener: (event: never) => void, options?: unknown): void;
  removeEventListener(type: string, listener: (event: never) => void, options?: unknown): void;
}

function defaultTarget(): ShortcutTarget {
  if (typeof window === 'undefined') {
    throw new Error('keyboard shortcuts require a DOM window');
  }
  return window as unknown as ShortcutTarget;
}

/**
 * Install the shortcut handler and return its cleanup.
 *
 * The listener is registered in the **capture** phase on `window` so an app
 * component cannot swallow a chord before it is seen, and so a keystroke aimed
 * at a focused field is still judged with that field as the event target (which
 * is what keeps text editing untouched). Call once per window; the returned
 * cleanup removes the listener and is safe to call more than once.
 */
export function installShortcutHandler(
  bindings: ShortcutBindings,
  target: ShortcutTarget = defaultTarget()
): () => void {
  const listener = (event: Event): void => {
    handleShortcut(event as unknown as ShortcutEventLike, bindings);
  };
  target.addEventListener('keydown', listener as (event: never) => void, { capture: true });

  let removed = false;
  return () => {
    if (removed) return;
    removed = true;
    target.removeEventListener('keydown', listener as (event: never) => void, {
      capture: true,
    });
  };
}