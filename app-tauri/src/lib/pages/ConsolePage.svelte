<script module lang="ts">
  /**
   * The four pad commands offered as one click.
   *
   * Same set, same order, and the same verbatim strings as the legacy GTK
   * console (`linux-app/ui/console_page.py`), all of them documented device
   * commands in the README table. Nothing else is quick-actionable: the
   * console only sends what the pad itself understands.
   */
  export const QUICK_COMMANDS = ['WHO', 'INFO', 'SAVE', 'HELP'] as const;

  /** Shown next to each quick button so the label alone never has to explain it. */
  export const QUICK_COMMAND_HINTS: Readonly<Record<string, string>> = {
    WHO: 'Ask the pad for its identity line',
    INFO: 'Dump the pad name, mode and timings',
    SAVE: 'Persist name, mode and timings on the pad',
    HELP: 'List the commands the pad supports',
  };
</script>

<script lang="ts">
  /**
   * Serial console: the live device log and the command line into the pad.
   *
   * The three rules this page lives by:
   *
   * 1. **One log.** Lines come from the shared `logEntries` buffer that
   *    `initializeAppState()` fills from `octopad://serial-log`. The page
   *    registers no listener of its own and keeps no second copy.
   * 2. **No invented successes.** `send_device_command_cmd` returns `true` only
   *    after a transport confirmed the write, and the manager's `[TX]` /
   *    `[TX-BLE]` line is the record of it. This page therefore never appends
   *    an echo of its own — a send that was refused leaves no line at all.
   * 3. **A failed send is visible and retryable.** The command runs through
   *    `runCommand`, so a refusal or an IPC error is shown next to the input and
   *    the text stays where it is.
   */
  import { get } from 'svelte/store';

  import ConsoleLog from '../components/ConsoleLog.svelte';
  import * as api from '../api';
  import { appState, clearLogs, lastError, logEntries, runCommand } from '../state';

  const snapshot = $derived($appState);

  const transport = $derived(
    snapshot === null
      ? 'Connecting…'
      : snapshot.serial.connected
        ? `Serial${snapshot.serial.port ? ` on ${snapshot.serial.port}` : ''}`
        : snapshot.ble.connected
          ? 'Bluetooth'
          : 'Not connected'
  );

  let commandInput = $state<HTMLInputElement | undefined>(undefined);
  let draft = $state('');
  let sending = $state(false);
  let sendError = $state<string | null>(null);

  /**
   * Send one command exactly as typed (trimmed, like the core trims it).
   *
   * Nothing is echoed locally: the only line a send produces is the `[TX]` /
   * `[TX-BLE]` record the manager emits once the write succeeded.
   */
  async function send(raw: string): Promise<void> {
    const command = raw.trim();
    if (command.length === 0) {
      // Nothing typed and nothing to retry — an empty line is not a failure.
      return;
    }

    sendError = null;
    sending = true;
    const accepted = await runCommand(`send ${command}`, () => api.sendDeviceCommand(command));
    sending = false;

    if (accepted) {
      draft = '';
      // The element as well as the draft: a `bind:value` update alone can be a
      // no-op when the field already holds the sent text.
      if (commandInput) commandInput.value = '';
      return;
    }

    // The write failed or was refused, so the pad never saw the command.
    sendError = get(lastError) ?? `The pad did not accept ${command}.`;
  }

  /** Enter in the field sends whatever is in it right now. */
  function submitDraft(): void {
    void send(commandInput?.value ?? draft);
  }

  function clearView(): void {
    clearLogs();
    sendError = null;
  }
</script>

<section class="page" data-page="console" data-testid="active-page">
  <header class="console__header">
    <div class="console__heading">
      <h1 class="page__title">Serial Console</h1>
      <p class="muted console__transport" data-testid="console-transport">{transport}</p>
    </div>

    <button type="button" class="btn" data-testid="console-clear" onclick={clearView}>
      Clear
    </button>
  </header>

  <div class="card">
    <h2 class="section-title">Device output</h2>
    <ConsoleLog entries={$logEntries} />
  </div>

  <div class="card">
    <h2 class="section-title">Send a command</h2>

    <div class="console__quick">
      {#each QUICK_COMMANDS as command (command)}
        <button
          type="button"
          class="btn"
          data-testid="quick-command"
          data-command={command}
          title={QUICK_COMMAND_HINTS[command]}
          disabled={sending}
          onclick={() => send(command)}
        >
          {command}
        </button>
      {/each}
    </div>

    <form
      class="console__send"
      onsubmit={(event) => {
        event.preventDefault();
        submitDraft();
      }}
    >
      <div class="console__field">
        <label class="console__label" for="consoleCommand">Command</label>
        <input
          id="consoleCommand"
          class="console__input mono"
          type="text"
          autocomplete="off"
          spellcheck="false"
          placeholder="NAME:Desk Pad"
          aria-invalid={sendError ? 'true' : undefined}
          aria-describedby={sendError ? 'console-send-error' : undefined}
          data-testid="console-input"
          bind:this={commandInput}
          bind:value={draft}
          onkeydown={(event) => {
            if (event.key === 'Enter') {
              // Enter sends once: cancelling the implicit submit keeps the
              // form and the key handler from both firing.
              event.preventDefault();
              submitDraft();
            }
          }}
        />
      </div>

      <button type="submit" class="btn btn--primary" disabled={sending} data-testid="console-send">
        Send
      </button>
    </form>

    {#if sendError}
      <p
        class="alert alert--error console__alert"
        id="console-send-error"
        role="alert"
        data-testid="console-error"
      >
        {sendError}
      </p>
    {/if}

    <p class="muted console__hint">
      Commands are written to the pad over whichever connection is live — USB serial or Bluetooth.
      The log line appears once the write actually reached it.
    </p>
  </div>
</section>

<style>
  .console__header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .console__heading {
    min-width: 0;
  }

  .console__transport {
    margin: 2px 0 0;
    font-size: var(--font-size-sm);
  }

  .console__quick {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .console__send {
    display: flex;
    align-items: flex-end;
    gap: var(--space-3);
    margin-top: var(--space-4);
    flex-wrap: wrap;
  }

  .console__field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    flex: 1 1 240px;
    min-width: 0;
  }

  .console__label {
    font-size: var(--font-size-sm);
    color: var(--text-muted);
  }

  .console__input {
    min-height: 30px;
    padding: 4px 8px;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background-color: var(--surface-base);
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: var(--font-size-sm);
  }

  .console__input[aria-invalid='true'] {
    border-color: var(--error);
  }

  .console__alert {
    margin: var(--space-3) 0 0;
  }

  .console__hint {
    margin: var(--space-4) 0 0;
    font-size: var(--font-size-sm);
  }

  @media (max-width: 640px) {
    .console__send {
      align-items: stretch;
      flex-direction: column;
    }
  }
</style>