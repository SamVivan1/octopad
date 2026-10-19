<script lang="ts">
  /**
   * Settings — placeholder until the timing controls and the device rename land
   * in a later milestone.
   */
  import { appState } from '../state';
  import { timingValue } from '../types';

  const snapshot = $derived($appState);
</script>

<section class="page" data-page="settings" data-testid="active-page">
  <h1 class="page__title">Settings</h1>

  <div class="card">
    <h2 class="section-title">Timings</h2>
    <p class="muted">
      Debounce {timingValue(snapshot?.config.debounceMs) ?? 'invalid'} ms · Click{' '}
      {timingValue(snapshot?.config.clickTimeoutMs) ?? 'invalid'} ms · Hold{' '}
      {timingValue(snapshot?.config.holdTimeoutMs) ?? 'invalid'} ms
    </p>
    <p class="muted settings__hint">
      Editable controls and the device rename field arrive with the settings page.
    </p>
  </div>
</section>

<style>
  .settings__hint {
    margin: var(--space-4) 0 0;
    font-size: var(--font-size-sm);
  }
</style>