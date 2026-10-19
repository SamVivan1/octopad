<script lang="ts">
  /**
   * Serial console — placeholder until the live log view and command input land
   * in the next milestone.
   */
  import { appState, logEntries } from '../state';

  const snapshot = $derived($appState);
</script>

<section class="page" data-page="console" data-testid="active-page">
  <h1 class="page__title">Serial Console</h1>

  <div class="card">
    <h2 class="section-title">Live log</h2>
    <p class="muted">
      {$logEntries.length} line{$logEntries.length === 1 ? '' : 's'} buffered. The timestamped log,
      quick commands and the command input arrive with the console page.
    </p>
    <p class="muted console__transport">
      Transport: {snapshot?.serial.connected ? 'serial' : snapshot?.ble.connected ? 'bluetooth' : 'none'}
    </p>
  </div>
</section>

<style>

  .console__transport {
    margin: var(--space-4) 0 0;
    font-size: var(--font-size-sm);
  }
</style>