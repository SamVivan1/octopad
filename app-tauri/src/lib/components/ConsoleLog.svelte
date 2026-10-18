<script lang="ts">
  import type { TimestampedLogLine } from '../types';

  export let logs: TimestampedLogLine[] = [];
  export let onClear: () => void;

  function classify(line: string): 'system' | 'action' | 'error' | 'tx' {
    const upper = line.toUpperCase();
    if (upper.includes('[ERROR') || upper.includes(' FAIL]')) return 'error';
    if (upper.includes('[ACTION]')) return 'action';
    if (upper.startsWith('[TX]') || upper.startsWith('[TX-BLE]') || line.startsWith('> ')) {
      return 'tx';
    }
    return 'system';
  }

  function formatTimestamp(timestamp: number): string {
    const date = new Date(timestamp);
    return [date.getHours(), date.getMinutes(), date.getSeconds()]
      .map((value) => String(value).padStart(2, '0'))
      .join(':');
  }
</script>

<section class="console-log" aria-label="Device log panel">
  <div class="log-heading">
    <h2>Device log</h2>
    <button type="button" aria-label="Clear" onclick={onClear}>Clear</button>
  </div>
  <div class="log-lines" role="log" aria-label="Device log" aria-live="polite">
    {#if logs.length === 0}
      <p class="empty-log">Device messages will appear here.</p>
    {:else}
      {#each logs as entry, index (`${entry.timestamp}-${index}`)}
        <div class={`log-line ${classify(entry.line)}`}>
          <time class="log-time" datetime={new Date(entry.timestamp).toISOString()}>
            {formatTimestamp(entry.timestamp)}
          </time>
          <span>{entry.line}</span>
        </div>
      {/each}
    {/if}
  </div>
</section>

<style>
  .console-log {
    display: grid;
    min-height: 18rem;
    grid-template-rows: auto minmax(0, 1fr);
    gap: var(--space-2);
  }

  .log-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    margin: 0;
    font-size: 1.05rem;
  }

  .log-heading button {
    min-height: 2.25rem;
    padding: 0 var(--space-3);
  }

  .log-lines {
    min-height: 16rem;
    max-height: 65vh;
    overflow: auto;
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-base);
    font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
    font-size: 0.85rem;
  }

  .log-line {
    display: grid;
    grid-template-columns: 4.5rem minmax(0, 1fr);
    gap: var(--space-2);
    padding: 0.1rem 0;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .log-time {
    color: var(--text-muted);
  }

  .system {
    color: var(--success);
  }

  .action {
    color: var(--trigger-double);
  }

  .error {
    color: var(--error);
  }

  .tx {
    color: var(--warning);
  }

  .empty-log {
    margin: 0;
    color: var(--text-muted);
  }
</style>
