<script lang="ts">
  import { sendDeviceCommand } from '../api';
  import { appState } from '../state';
  import ConsoleLog from '../components/ConsoleLog.svelte';

  let commandDraft = '';
  let sending = false;
  let feedback = '';

  $: logs = $appState.logs;

  async function sendCommand(command = commandDraft) {
    const text = command.trim();
    if (!text || sending) return;

    commandDraft = text;
    sending = true;
    feedback = '';
    try {
      if (!(await sendDeviceCommand(text))) {
        feedback = 'Command send failed. Check the device connection and try again.';
        return;
      }
      commandDraft = '';
    } catch (error) {
      feedback = `Command send failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      sending = false;
    }
  }

  function sendQuickCommand(command: string) {
    commandDraft = command;
    void sendCommand(command);
  }

  function onCommandKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      void sendCommand();
    }
  }

  function clearLogs() {
    appState.update((current) => ({ ...current, logs: [] }));
  }
</script>

<section class="console-page" aria-labelledby="console-title">
  <div class="page-heading">
    <div>
      <h1 id="console-title">Serial Console</h1>
      <p>Live device messages and command input.</p>
    </div>
    <div class="quick-commands" aria-label="Quick commands">
      {#each ['WHO', 'INFO', 'SAVE', 'HELP'] as command}
        <button type="button" disabled={sending} onclick={() => sendQuickCommand(command)}>
          {command}
        </button>
      {/each}
    </div>
  </div>

  {#if feedback}
    <p class="feedback" role="alert">{feedback}</p>
  {/if}

  <form
    class="command-form"
    onsubmit={(event) => {
      event.preventDefault();
      void sendCommand();
    }}
  >
    <label for="device-command">Command</label>
    <div class="command-row">
      <input
        id="device-command"
        aria-label="Command"
        bind:value={commandDraft}
        disabled={sending}
        onkeydown={onCommandKeydown}
        autocomplete="off"
        spellcheck="false"
      />
      <button type="submit" disabled={sending || !commandDraft.trim()}>Send</button>
    </div>
  </form>

  <ConsoleLog {logs} onClear={clearLogs} />
</section>

<style>
  .console-page {
    display: grid;
    gap: var(--space-4);
  }

  .page-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  h1 {
    margin: 0 0 var(--space-1);
    font-size: 1.5rem;
  }

  .page-heading p {
    margin: 0;
    color: var(--text-muted);
  }

  .quick-commands {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .quick-commands button {
    min-height: 2.5rem;
    padding: 0 var(--space-3);
  }

  .command-form {
    display: grid;
    gap: var(--space-2);
  }

  .command-form label {
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .command-row {
    display: flex;
    gap: var(--space-2);
  }

  .command-row input {
    flex: 1;
    min-width: 0;
    min-height: 2.75rem;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-base);
  }

  .command-row button {
    min-width: 5rem;
    padding: 0 var(--space-3);
  }

  .feedback {
    margin: 0;
    color: var(--error);
  }
</style>
