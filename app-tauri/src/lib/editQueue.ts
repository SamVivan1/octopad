/**
 * The one serialized command queue for config edits.
 *
 * Every mutation the UI sends — a mode switch, a key selection, a label, an
 * action edit, a timing — travels through {@link runEdit}, in the order the
 * user produced it. That is what the M2 Review Focus calls "rapid mode/button
 * selection": an edit issued right after a click must not overtake the
 * selection command it depends on and land on a different key or mode.
 *
 * Two properties matter and are easy to get wrong:
 *
 * - **A failed command never breaks the chain.** The queue continues with the
 *   next command whether the previous one resolved or rejected, so one refused
 *   selection cannot freeze the UI for the rest of the session.
 * - **It holds promises, not results.** Nothing here interprets a `false`
 *   return: `runCommand` in `./state` does that, and it is what turns a refusal
 *   into visible feedback.
 *
 * The queue is module state on purpose — it is one queue for the whole app, not
 * one per page, so a page switch mid-flight cannot reorder anything.
 */

let queue: Promise<unknown> = Promise.resolve();

/**
 * Run `task` once every previously queued command has settled.
 *
 * Resolves or rejects with `task`'s own outcome, so callers can still react to a
 * failure instead of having it swallowed here.
 */
export function runEdit<T>(task: () => Promise<T>): Promise<T> {
  const next = queue.then(task, task);
  queue = next.then(
    () => undefined,
    () => undefined
  );
  return next;
}

/** Drop the pending chain (tests, and a full app reset). */
export function resetEditQueue(): void {
  queue = Promise.resolve();
}