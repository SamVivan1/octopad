<script module lang="ts">
  /**
   * How one buffered line is presented.
   *
   * - `tx` — the manager's `[TX] …` / `[TX-BLE] …` record. It is emitted only
   *   after a transport confirmed the write, so it is the *only* evidence that
   *   a command reached the pad; the console never adds a line of its own.
   * - `device` — a line the pad itself produced: a documented reply
   *   (`HELLO:…`, `INFO:…`, `MODE: …`) or a button press.
   * - `error` — a failure the core reported (`[ERROR] …`, `[BLE FAIL] …`, or an
   *   action that ended in `[FAIL]`).
   * - `system` — anything else, i.e. host-side chatter.
   */
  export type ConsoleLineKind = 'tx' | 'device' | 'error' | 'system';

  /** The view never grows past this many lines. */
  export const VISIBLE_LIMIT = 1500;

  /** How many of the oldest lines are dropped when the limit is passed. */
  export const TRIM_BATCH = 200;

  /** `TAG:` replies: `HELLO:OCTOPAD:…`, `INFO:…`, `NAME:OK:…`, `RESET:OK`. */
  const DEVICE_REPLY = /^[A-Za-z][A-Za-z0-9]*:/;

  /** The bare replies in the documented command table. */
  const DEVICE_BARE_REPLY = /^(?:SAVED|REBOOTING)$/i;

  /** A key press, as `bridge::connect_device_events` logs it. */
  const DEVICE_EVENT = /^\[(?:DESKTOP|HA)]\s+Button\s+\d+\s*->\s*(?:SINGLE|DOUBLE|HOLD)$/i;

  /** The manager's write record for either transport. */
  const TX_RECORD = /^\[TX(?:-BLE)?]/i;

  /**
   * The outcome of one executed action, as `exec::chain` logs it:
   * `[ACTION] B<k> (<TRIGGER>): <reason> [OK|FAIL]`.
   *
   * The trailing `[FAIL]` is bracketed, so the loose `' FAIL]'` test below — the
   * one that catches `[BLE FAIL] …` — cannot see it. Matched in full, group 1
   * being the outcome, so an action that failed is never rendered as chatter.
   */
  const ACTION_OUTCOME = /^\[ACTION]\s+B\d+\s+\([A-Z]+\):.*\[(OK|FAIL)]$/i;

  /**
   * Classify one line for display.
   *
   * Errors win over everything else so a failure can never be rendered as an
   * ordinary line, and the TX record is checked before the pad's own vocabulary
   * because `[TX-BLE] INFO` is a write, not an `INFO:` reply.
   */
  export function classifyLogLine(line: string): ConsoleLineKind {
    // An action reports its own outcome, so it is classified by that outcome and
    // not by the text of the reason: `[OK]` is an ordinary line, `[FAIL]` a
    // failure, whatever the reason happens to contain.
    const action = ACTION_OUTCOME.exec(line);
    if (action) return action[1]?.toUpperCase() === 'FAIL' ? 'error' : 'system';

    const upper = line.toUpperCase();
    if (upper.includes('[ERROR') || upper.includes(' FAIL]')) {
      return 'error';
    }
    if (TX_RECORD.test(line)) {
      return 'tx';
    }
    if (DEVICE_REPLY.test(line) || DEVICE_BARE_REPLY.test(line) || DEVICE_EVENT.test(line)) {
      return 'device';
    }
    return 'system';
  }

  /** `HH:MM:SS` in local time, the same stamp the legacy GTK console wrote. */
  export function formatLogTime(receivedAt: number): string {
    const at = new Date(receivedAt);
    const pad = (value: number): string => String(value).padStart(2, '0');
    return `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}`;
  }
</script>

<script lang="ts">
  /**
   * The live log view: pure presentation over the shared `logEntries` buffer.
   *
   * It owns no lines of its own. Everything rendered comes from the buffer
   * `initializeAppState()` fills from `octopad://serial-log`, so navigating away
   * and back shows the same history the shell already had.
   *
   * **Bounded view.** The shared store keeps the newest {@link VISIBLE_LIMIT}
   * lines; on top of that the view never *displays* more than the same number.
   * Once the window holds more than that, the oldest {@link TRIM_BATCH} lines
   * are dropped in one go, so a chatty pad cannot grow the DOM without limit.
   * The window is only two numbers (`windowStart` and the newest id seen), never
   * a second copy of the log.
   */
  import type { LogEntry } from '../state';

  interface Props {
    entries: LogEntry[];
  }

  let { entries }: Props = $props();

  /** Id of the newest entry the window starts counting from. */
  let windowStart = $state(0);
  /** Newest entry id seen so far, so a Clear or a reload starts a new window. */
  let newestSeen = $state(-1);
  let viewport = $state<HTMLElement | undefined>(undefined);

  $effect(() => {
    const newest = entries.length > 0 ? entries[entries.length - 1].id : -1;

    // The buffer restarted (Clear, or ids that went backwards after a reload):
    // everything currently in it belongs to the new window.
    if (newest < newestSeen) {
      windowStart = 0;
      newestSeen = newest;
      return;
    }
    newestSeen = newest;

    // Lines delivered into the current window; ids are monotonic, so this is
    // exact even when several events land in one flush.
    const delivered = newest + 1 - windowStart;
    if (delivered > VISIBLE_LIMIT) {
      windowStart += TRIM_BATCH;
    }
  });

  const visible = $derived.by(() => {
    if (windowStart <= 0) return entries;
    const first = entries.findIndex((entry) => entry.id >= windowStart);
    return first < 0 ? [] : entries.slice(first);
  });

  // Follow the tail: a console that does not scroll is a console you have to
  // drag back to the newest line.
  $effect(() => {
    void visible.length;
    if (viewport) {
      viewport.scrollTop = viewport.scrollHeight;
    }
  });
</script>

<div class="console-log" data-testid="console-log" bind:this={viewport}>
  {#if visible.length === 0}
    <p class="muted console-log__empty" data-testid="console-empty">
      Nothing from the pad yet. It answers on the connection you already have open.
    </p>
  {:else}
    {#each visible as entry (entry.id)}
      {@const kind = classifyLogLine(entry.line)}
      <div
        class="console-log__line console-log__line--{kind}"
        data-kind={kind}
        data-line={entry.line}
        data-testid="log-line"
      >
        <span class="console-log__time mono" data-testid="log-time">
          {formatLogTime(entry.receivedAt)}
        </span>
        <span class="console-log__text mono" data-testid="log-text">{entry.line}</span>
      </div>
    {/each}
  {/if}
</div>

<style>
  /* Only design tokens, so a second theme never needs these rules rewritten. */
  .console-log {
    max-height: 420px;
    min-height: 120px;
    overflow-y: auto;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background-color: var(--surface-sunken);
  }

  .console-log__empty {
    margin: 0;
    font-size: var(--font-size-sm);
  }

  .console-log__line {
    display: flex;
    gap: var(--space-3);
    padding: 1px 0;
    font-family: var(--font-mono);
    font-size: var(--font-size-sm);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .console-log__time {
    flex: none;
    color: var(--text-faint);
  }

  .console-log__text {
    min-width: 0;
  }

  /* Pad replies: the interesting traffic. */
  .console-log__line--device .console-log__text {
    color: var(--text-primary);
  }

  /* The manager's `[TX]` / `[TX-BLE]` write record. */
  .console-log__line--tx .console-log__text {
    color: var(--warning);
  }

  /* Anything the core reported as a failure. */
  .console-log__line--error .console-log__text {
    color: var(--error);
    font-weight: 600;
  }

  /* Host chatter that needs no attention. */
  .console-log__line--system .console-log__text {
    color: var(--text-secondary);
  }
</style>